//! S2 spike dummy host (§11.3): the running "application" that gets hot-patched.
//!
//! The host exports the same well-known symbols as `patch-tip` (v0 bodies).
//! The spike driver (`xtask patch-spike`) builds the tip crate as a cdylib
//! with `LIG_TIP_LABEL`/`LIG_TIP_COUNT` env-selected bodies, then runs this
//! host with `LIG_PATCH_DYLIB` pointing at the fresh dylib. The host:
//!
//! 1. asserts the **miss path** (un-patched calls return v0),
//! 2. applies the patch dylib through `lig_patch::apply_patch` (jump table
//!    commit; the dylib is never `dlclose`d),
//! 3. asserts the **routed frames** (patched calls return the new bodies),
//! 4. round-trips the patch manifest,
//! 5. prints the measured in-process save→frame cost
//!    (`apply_ns`, `frame_ns`) and exits non-zero on any violation.
//!
//! The end-to-end budget (tip recompile + relink + apply + first frame < 1 s)
//! is enforced by the xtask gate, which owns the wall clock.

#![deny(missing_docs)]
// The exported patchable symbols must carry stable names for the jump-table
// protocol (`dlsym` by name, §2.4). `#[no_mangle]` is the whole point of this
// binary; nothing else here touches unsafe.
#![allow(unsafe_code)]

use std::hint::black_box;
use std::time::Instant;

use lig_patch::{FRAME_COUNT, FRAME_LABEL, PatchManifest, apply_patch, hot_call};

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

fn main() {
    let dylib =
        std::env::var("LIG_PATCH_DYLIB").expect("LIG_PATCH_DYLIB must point at the tip dylib");

    // 1. Miss path: before any patch, safepoint calls land in the fat build.
    let label: fn() -> &'static str = patchable_frame_label;
    let count: fn() -> u64 = patchable_frame_count;
    assert_eq!(hot_call(label), "v0", "miss path must hit the fat build");
    assert_eq!(hot_call(count), 0, "miss path must hit the fat build");

    // 2. Apply: load dylib, resolve symbols, rebase, commit the jump table.
    let t_apply = Instant::now();
    let receipt = apply_patch(std::path::Path::new(&dylib), &[FRAME_LABEL, FRAME_COUNT])
        .expect("patch applies");
    let apply_ns = t_apply.elapsed().as_nanos();

    // 3. Routed frames: the first patched call completes save→frame.
    let t_frame = Instant::now();
    let routed_label = black_box(hot_call(black_box(label)));
    let first_frame_ns = t_frame.elapsed().as_nanos();
    let t_steady = Instant::now();
    let routed_count = black_box(hot_call(black_box(count)));
    let steady_frame_ns = t_steady.elapsed().as_nanos();

    assert_eq!(routed_label, "v1", "routed frame must hit the patched body");
    assert_eq!(routed_count, 42, "routed frame must hit the patched body");
    assert_eq!(hot_call(label), "v1", "steady state stays routed");
    assert!(receipt.mapped >= 2, "both symbols must be remapped");

    // 4. Manifest round-trip (§11.3 step 3): receipt → toml → parse.
    let manifest = PatchManifest::from_receipt(std::path::Path::new(&dylib), 1, &receipt);
    let parsed = PatchManifest::parse(&manifest.to_toml()).expect("manifest round-trips");
    assert_eq!(parsed, manifest, "manifest serialization is lossless");
    assert_eq!(parsed.entries.len(), receipt.file_relative.len());

    // 5. Report: machine-readable lines for the xtask gate.
    println!("patch_mapped={}", receipt.mapped);
    println!("patch_dylib={}", receipt.dylib.display());
    println!("apply_ns={apply_ns}");
    println!("frame_ns={first_frame_ns}");
    println!("frame2_ns={steady_frame_ns}");
    let first = receipt
        .file_relative
        .iter()
        .next()
        .map_or_else(String::new, |(k, v)| format!("{k}={v:#x}"));
    println!("file_relative_first={first}");
    println!("host_result=PASS");
}
