//! S2 spike dummy host (§11.3): the running "application" that gets hot-patched.
//!
//! The host exports the same well-known symbols as `patch-tip` (v0 bodies;
//! `patchable_frame_extra` is the M3 **stub thunk** placeholder). The spike
//! driver (`lig patch-spike`) builds TWO tip generations as cdylibs and runs
//! this host with `LIG_PATCH_DYLIB` (v1) and `LIG_PATCH_DYLIB2` (v2). The
//! M3 ThinLink flow:
//!
//! 1. asserts the **miss path** (un-patched calls return v0 — including the
//!    stub thunk),
//! 2. applies generation 1 through `lig_patch::PatchSession` (jump-table
//!    commit; the dylib is never `dlclose`d),
//! 3. asserts generation 1's routed frames and captures the committed
//!    (runtime) address of the routed label,
//! 4. applies generation 2 — which also fills the **stub thunk** — and
//!    asserts the new bodies plus the chained manifest history,
//! 5. calls the generation-1 captured address directly: still executable,
//!    still v1 — the never-dlclose / stale-frame-safety proof (§2.4),
//! 6. round-trips both manifests and prints the measured in-process
//!    save→frame cost per generation (`apply_ns`, `frame_ns`, `apply2_ns`,
//!    `frame2_ns`, `stub_ns`), exiting non-zero on any violation.
//!
//! The end-to-end budget (tip recompile + relink + apply + first frame < 1 s
//! per save) is enforced by the `lig patch-spike` gate, which owns the wall
//! clock.

#![deny(missing_docs)]
// The exported patchable symbols must carry stable names for the jump-table
// protocol (`dlsym` by name, §2.4). `#[no_mangle]` is the whole point of this
// binary; the captured-address call is the documented never-dlclose proof.
#![allow(unsafe_code)]

use std::hint::black_box;
use std::time::Instant;

use lig_patch::{FRAME_COUNT, FRAME_LABEL, PatchSession, hot_call};

/// The stub-thunk symbol name (matches `patch-tip`'s third export).
pub const FRAME_EXTRA: &str = "patchable_frame_extra";

/// v0 body — replaced (via the jump table) by the patched dylib.
#[unsafe(no_mangle)]
pub extern "Rust" fn patchable_frame_label() -> &'static str {
    "v0"
}

/// v0 body for the second patchable symbol (multi-symbol map proof).
#[unsafe(no_mangle)]
pub extern "Rust" fn patchable_frame_count() -> u64 {
    0
}

/// The fat build's **stub thunk** for a function a later generation
/// introduces (ADR-013: "new-symbol registration … the full thunk story
/// lands with M3"). Returns a placeholder until a patch routes it.
#[unsafe(no_mangle)]
pub extern "Rust" fn patchable_frame_extra() -> u64 {
    0
}

fn main() {
    let dylib1 =
        std::env::var("LIG_PATCH_DYLIB").expect("LIG_PATCH_DYLIB must point at the tip dylib");
    let dylib2 = std::env::var("LIG_PATCH_DYLIB2")
        .expect("LIG_PATCH_DYLIB2 must point at the second tip generation");

    // 1. Miss path: before any patch, safepoint calls land in the fat build.
    let label: fn() -> &'static str = patchable_frame_label;
    let count: fn() -> u64 = patchable_frame_count;
    let extra: fn() -> u64 = patchable_frame_extra;
    assert_eq!(hot_call(label), "v0", "miss path must hit the fat build");
    assert_eq!(hot_call(count), 0, "miss path must hit the fat build");
    assert_eq!(
        hot_call(extra),
        0,
        "stub thunk must serve v0 before the patch"
    );

    let mut session = PatchSession::new();

    // 2. Generation 1: load, resolve, rebase, commit the jump table.
    let t_apply = Instant::now();
    let gen1 = session
        .apply(
            std::path::Path::new(&dylib1),
            &[FRAME_LABEL, FRAME_COUNT],
            &[],
        )
        .expect("generation 1 applies");
    let apply_ns = t_apply.elapsed().as_nanos();

    // 3. Routed frames: the first patched call completes save→frame.
    let t_frame = Instant::now();
    let routed_label = black_box(hot_call(black_box(label)));
    let first_frame_ns = t_frame.elapsed().as_nanos();
    assert_eq!(routed_label, "v1", "routed frame must hit the patched body");
    assert_eq!(hot_call(count), 42, "generation 1 body is live");
    assert_eq!(
        hot_call(extra),
        0,
        "the stub thunk is untouched until a patch requests it"
    );
    assert_eq!(gen1.build_id, 1, "the first apply is build id 1");

    // Capture the generation-1 committed address of the routed label — what
    // a stale frame inside generation 1 would return into.
    let gen1_addr = lig_patch::captured_address(label).expect("generation 1 table entry");

    // 4. Generation 2: also fills the stub thunk.
    let t_apply2 = Instant::now();
    let gen2 = session
        .apply(
            std::path::Path::new(&dylib2),
            &[FRAME_LABEL, FRAME_COUNT, FRAME_EXTRA],
            &[FRAME_EXTRA],
        )
        .expect("generation 2 applies");
    let apply_two_ns = t_apply2.elapsed().as_nanos();

    let t_frame2 = Instant::now();
    let routed_label2 = black_box(hot_call(black_box(label)));
    let second_frame_ns = t_frame2.elapsed().as_nanos();
    let t_stub = Instant::now();
    let routed_extra = black_box(hot_call(black_box(extra)));
    let stub_ns = t_stub.elapsed().as_nanos();
    assert_eq!(routed_label2, "v2", "generation 2 body is live");
    assert_eq!(hot_call(count), 43, "generation 2 body is live");
    assert_eq!(routed_extra, 7, "the stub thunk now routes to the patch");
    assert_eq!(
        gen2.build_id, 2,
        "the session chains generations monotonically"
    );

    // 5. Never-dlclose proof: the generation-1 address is still mapped and
    // still runs the generation-1 body (a stale frame survives, §2.4).
    let stale_body = black_box(lig_patch::call_captured::<&'static str>(gen1_addr));
    assert_eq!(stale_body, "v1", "generation 1 code stays executable");
    assert_ne!(
        gen1_addr, label as usize as u64,
        "the captured address is the patch, not the fat build"
    );

    // 6. Manifest chain (§11.3 step 3): receipt → toml → parse, per generation.
    for (i, applied) in session.applied().iter().enumerate() {
        let parsed = lig_patch::PatchManifest::parse(&applied.manifest.to_toml())
            .expect("manifest round-trips");
        assert_eq!(parsed, applied.manifest, "manifest {i} is lossless");
        assert_eq!(parsed.build_id, i as u64 + 1, "build ids chain 1, 2");
    }
    assert_eq!(session.applied().len(), 2);
    assert_eq!(
        gen2.symbols.get(FRAME_EXTRA),
        Some(&lig_patch::SymbolKind::StubThunk),
        "the session classifies the stub thunk"
    );
    assert_eq!(
        gen1.symbols.get(FRAME_EXTRA),
        None,
        "generation 1 never requested the stub"
    );

    // 7. Report: machine-readable lines for the patch-spike gate.
    println!("patch_mapped={}", gen1.receipt.mapped);
    println!("patch_mapped2={}", gen2.receipt.mapped);
    println!("apply_ns={apply_ns}");
    println!("frame_ns={first_frame_ns}");
    println!("apply2_ns={apply_two_ns}");
    println!("frame2_ns={second_frame_ns}");
    println!("stub_ns={stub_ns}");
    println!("host_result=PASS");
}
