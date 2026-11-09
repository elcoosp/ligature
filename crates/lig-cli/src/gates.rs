//! CI gates (§13) as native `lig` subcommands: fixture generation, harness
//! verification, reproducibility byte-diffs, budget checks. These were the
//! `xtask` commands through M1; M2 folds them into the CLI so the gate tool
//! ships with the compiler binary (no external dependencies — the gate must
//! never be the slow part).

use lig_codegen::generate_component;
use lig_core::id::ComponentId;
use lig_parse_solid::tests_support::COUNTER_TSX;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The workspace root, resolved from this crate's location.
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn counter_module() -> lig_ir::ViewModule {
    lig_parse_solid::lower_module(COUNTER_TSX, "counter.tsx").expect("counter lowers")
}

/// Regenerate the committed generated-code artifacts (`lig fixtures gen`).
pub fn fixtures_gen() -> i32 {
    let root = repo_root();
    let module = counter_module();

    // Generated Rust for the counter component (S1 demo + compile-clean gate).
    let unit = match generate_component(&module, ComponentId::new(0)) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("codegen failed: {e}");
            return 1;
        }
    };
    let out_dir = root.join("fixtures/generated");
    std::fs::create_dir_all(&out_dir).expect("fixtures/generated exists");
    let out_path = out_dir.join("counter.rs");
    std::fs::write(&out_path, &unit.text).expect("write counter.rs");

    // Whole-module generated Rust for every conformance fixture → the
    // compile-clean crate (§13 compile-clean gate + KR-1 demo).
    let gen_dir = root.join("tests/compile-clean/src/gen");
    std::fs::create_dir_all(&gen_dir).expect("compile-clean gen dir");
    for (name, dir) in conformance_dirs(&root) {
        let source = std::fs::read_to_string(dir.join("input.solid.tsx"))
            .unwrap_or_else(|e| panic!("read {name}: {e}"));
        let m = match lig_parse_solid::lower_module(&source, &format!("{name}.tsx")) {
            Ok(m) => m,
            Err(diags) => {
                eprintln!("fixtures gen: {name} failed admission:");
                for d in diags {
                    eprintln!("  {d}");
                }
                return 1;
            }
        };
        let text = match lig_codegen::generate_module(&m) {
            Ok(g) => g.text,
            Err(e) => {
                eprintln!("fixtures gen: {name} codegen failed: {e}");
                return 1;
            }
        };
        let mod_name = name.replace('-', "_");
        std::fs::write(gen_dir.join(format!("{mod_name}.rs")), text)
            .unwrap_or_else(|e| panic!("write {mod_name}.rs: {e}"));
        println!(
            "generated: {}",
            gen_dir.join(format!("{mod_name}.rs")).display()
        );
    }

    // Canonical expected IR for the counter conformance fixture (S4).
    // Same serialization as `fixtures parity-gen` — one canonical form,
    // trailing newline included, so the two regenerators never disagree.
    let conf_dir = root.join("fixtures/conformance/counter");
    std::fs::create_dir_all(&conf_dir).expect("conformance dir");
    std::fs::write(conf_dir.join("input.solid.tsx"), COUNTER_TSX).expect("write solid input");
    std::fs::write(
        conf_dir.join("expected.ir.json"),
        format!("{}\n", module.canonical_json_pretty()),
    )
    .expect("write expected ir");
    println!("generated: {}", out_path.display());
    println!("generated: {}", conf_dir.join("expected.ir.json").display());
    0
}

/// D-8 gate: the committed generated file must equal a fresh generation
/// (`lig reproducibility`).
pub fn reproducibility() -> i32 {
    let root = repo_root();
    let module = counter_module();
    let unit = match generate_component(&module, ComponentId::new(0)) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("codegen failed: {e}");
            return 1;
        }
    };
    let committed = root.join("fixtures/generated/counter.rs");
    if !committed.exists() {
        eprintln!("missing {}; run `lig fixtures gen`", committed.display());
        return 1;
    }
    let old = std::fs::read_to_string(&committed).expect("read committed counter.rs");
    if old == unit.text {
        println!("reproducibility: OK (byte-identical, {} bytes)", old.len());
        0
    } else {
        eprintln!("reproducibility: FAIL — generated output drifted");
        eprintln!("  run `lig fixtures gen` and review the diff");
        1
    }
}

// ---------------------------------------------------------------------------
// S4: canonical-JSON parity harness (§4, §17.1)
// ---------------------------------------------------------------------------

/// Every conformance fixture dir (a dir under `fixtures/conformance` that
/// contains `input.solid.tsx`), in stable order.
pub fn conformance_dirs(root: &Path) -> Vec<(String, PathBuf)> {
    let base = root.join("fixtures/conformance");
    let mut dirs: Vec<(String, PathBuf)> = std::fs::read_dir(&base)
        .unwrap_or_else(|e| panic!("read {}: {e}", base.display()))
        .filter_map(|entry| {
            let dir = entry.ok()?.path();
            if dir.is_dir() && dir.join("input.solid.tsx").exists() {
                let name = dir.file_name()?.to_string_lossy().into_owned();
                Some((name, dir))
            } else {
                None
            }
        })
        .collect();
    dirs.sort();
    dirs
}

/// Lower one Solid fixture; fail-closed on any error diagnostic.
fn lower_solid_fixture(name: &str, dir: &Path) -> Result<lig_ir::ViewModule, i32> {
    let source = std::fs::read_to_string(dir.join("input.solid.tsx"))
        .unwrap_or_else(|e| panic!("read {name}/input.solid.tsx: {e}"));
    match lig_parse_solid::lower_module(&source, &format!("{name}.tsx")) {
        Ok(m) => Ok(m),
        Err(diags) => {
            eprintln!(
                "parity: {name} failed admission ({} diagnostics):",
                diags.len()
            );
            for d in diags {
                eprintln!("  {d}");
            }
            Err(1)
        }
    }
}

/// Lower one Vue fixture; fail-closed on any error diagnostic. A missing
/// `input.vue` is an error — the M2 contract requires both legs
/// (fixtures/README.md).
fn lower_vue_fixture(name: &str, dir: &Path) -> Result<lig_ir::ViewModule, String> {
    let path = dir.join("input.vue");
    let source = std::fs::read_to_string(&path)
        .map_err(|_| "missing input.vue — the M2 contract requires both legs".to_string())?;
    lig_parse_vue::lower_module(&source, &format!("{name}.vue")).map_err(|diags| {
        format!(
            "{} diagnostic(s):\n{}",
            diags.len(),
            diags
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        )
    })
}

/// The parity view of a canonical JSON: the module with the parity-ignored
/// fields normalized (§6.4) — the `frontend` header, the source file's
/// identity, and every span (expr spans, dynamic spans, lint positions, the
/// span table). Field paths are explicit: `reads`, fragment `children`, and
/// every id stay parity-relevant.
fn parity_view(json: &str) -> serde_json::Value {
    let mut v: serde_json::Value =
        serde_json::from_str(json).expect("canonical JSON parses as serde_json::Value");
    let Some(obj) = v.as_object_mut() else {
        return v;
    };
    obj.insert("frontend".into(), serde_json::json!("*"));
    obj.insert(
        "source_files".into(),
        serde_json::json!([{ "id": 0, "path": "*", "hash": 0 }]),
    );
    if let Some(serde_json::Value::Object(spans)) = obj.get_mut("spans") {
        spans.insert("entries".into(), serde_json::json!([]));
    }
    if let Some(serde_json::Value::Object(exprs)) = obj.get_mut("exprs")
        && let Some(serde_json::Value::Array(spans)) = exprs.get_mut("spans")
    {
        for s in spans.iter_mut() {
            *s = serde_json::json!({ "start": 0, "end": 0 });
        }
    }
    if let Some(serde_json::Value::Array(frags)) = obj.get_mut("fragments") {
        for f in frags.iter_mut() {
            if let Some(serde_json::Value::Array(dyns)) = f.get_mut("dynamics") {
                for d in dyns.iter_mut() {
                    if d.is_object() {
                        d["span"] = serde_json::json!([0, 0]);
                    }
                }
            }
        }
    }
    if let Some(serde_json::Value::Array(lints)) = obj.get_mut("lints") {
        for l in lints.iter_mut() {
            if l.is_object() {
                l["start"] = serde_json::json!(0);
                l["end"] = serde_json::json!(0);
            }
        }
    }
    v
}

/// Regenerate `expected.ir.json` for every fixture from the current frontend
/// (`lig fixtures parity-gen`).
///
/// Review the diff before committing: these files are the canonical contract
/// the `fixtures verify` gate enforces (golden files, §4).
pub fn parity_gen() -> i32 {
    let root = repo_root();
    let dirs = conformance_dirs(&root);
    if dirs.len() < 12 {
        eprintln!(
            "parity-gen: expected ≥12 behavior fixtures (§17.1), found {}",
            dirs.len()
        );
        return 1;
    }
    let mut failures = Vec::new();
    for (name, dir) in &dirs {
        match lower_solid_fixture(name, dir) {
            Ok(module) => {
                let json = format!("{}\n", module.canonical_json_pretty());
                let out = dir.join("expected.ir.json");
                std::fs::write(&out, json)
                    .unwrap_or_else(|e| panic!("write {}: {e}", out.display()));
                println!("parity-gen: {name} → {}", out.display());
            }
            Err(_) => failures.push(name.clone()),
        }
    }
    if failures.is_empty() {
        println!("parity-gen: {} fixtures regenerated", dirs.len());
        0
    } else {
        eprintln!("parity-gen: {} fixture(s) failed admission", failures.len());
        1
    }
}

/// S4 gate (`lig fixtures verify`): every fixture must match the committed
/// canonical contract on BOTH legs — the Solid leg byte-exact, the Vue leg
/// modulo the parity-ignored fields (§6.4: the `frontend` header, the source
/// file's identity, and per-source spans are frontend-owned bookkeeping).
pub fn parity() -> i32 {
    let root = repo_root();
    let dirs = conformance_dirs(&root);
    if dirs.len() < 12 {
        eprintln!(
            "parity: expected ≥12 behavior fixtures (§17.1), found {}",
            dirs.len()
        );
        return 1;
    }
    let mut failed = 0usize;
    for (name, dir) in &dirs {
        let expected_path = dir.join("expected.ir.json");
        let Ok(expected) = std::fs::read_to_string(&expected_path) else {
            eprintln!("parity: {name} missing expected.ir.json — run fixtures parity-gen");
            failed += 1;
            continue;
        };
        let mut leg_failed = false;
        match lower_solid_fixture(name, dir) {
            Ok(module) => {
                // Trailing-newline tolerant: editors and writers must not
                // flip the gate; everything inside is byte-exact.
                let actual = module.canonical_json_pretty();
                let expected = expected.trim_end_matches('\n');
                if actual == expected {
                    println!("parity: {name} OK");
                } else {
                    eprintln!(
                        "parity: {name} DRIFT — canonical JSON differs from committed contract"
                    );
                    leg_failed = true;
                }
            }
            Err(code) => {
                failed += 1;
                let _ = code;
                leg_failed = true;
            }
        }
        // The Vue leg (M2).
        match lower_vue_fixture(name, dir) {
            Ok(module) => {
                let actual = parity_view(&module.canonical_json_pretty());
                let expected = parity_view(expected.trim_end_matches('\n'));
                if actual == expected {
                    println!("parity: {name} vue leg OK");
                } else {
                    eprintln!(
                        "parity: {name} vue leg DRIFT — differs from the shared contract modulo the parity-ignored fields"
                    );
                    leg_failed = true;
                }
            }
            Err(e) => {
                eprintln!("parity: {name} vue leg failed: {e}");
                leg_failed = true;
            }
        }
        if leg_failed {
            failed += 1;
        }
    }
    if failed == 0 {
        println!(
            "parity: all {} fixtures match the committed canonical IR (both legs)",
            dirs.len()
        );
        0
    } else {
        eprintln!("parity: {failed} fixture(s) drifted or failed");
        1
    }
}

// ---------------------------------------------------------------------------
// S2: end-to-end hot-patch spike (§11.3, §17.1) — < 1 s save→frame
// ---------------------------------------------------------------------------

/// The tip dylib artifact for the current platform.
fn tip_dylib(tip_target: &Path) -> PathBuf {
    let name = if cfg!(target_os = "macos") {
        "libpatch_tip.dylib"
    } else {
        "libpatch_tip.so"
    };
    tip_target.join("debug").join(name)
}

/// Run a `cargo` subcommand in `dir` with extra env, inheriting stdio on
/// failure; returns true on success. Shared by the patch-spike and no-std
/// gates.
pub(crate) fn run_cargo(
    dir: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
    capture: bool,
) -> Result<String, i32> {
    let mut cmd = std::process::Command::new("cargo");
    cmd.current_dir(dir).args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    if capture {
        let out = cmd
            .output()
            .unwrap_or_else(|e| panic!("spawn cargo {args:?}: {e}"));
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            eprint!("{}", String::from_utf8_lossy(&out.stderr));
            Err(out.status.code().unwrap_or(1))
        }
    } else {
        let status = cmd
            .status()
            .unwrap_or_else(|e| panic!("spawn cargo {args:?}: {e}"));
        if status.success() {
            Ok(String::new())
        } else {
            Err(status.code().unwrap_or(1))
        }
    }
}

/// S2 gate (`lig patch-spike`): the subsecond-shaped save→frame budget on a
/// dummy host.
///
/// Pipeline (clock starts at the "save", i.e. the tip rebuild):
/// 1. rebuild `patch-tip` as a cdylib with v1 bodies (`LIG_TIP_LABEL=v1`,
///    `LIG_TIP_COUNT=42`),
/// 2. run `dummy-host`, which applies the dylib through `lig-patch` and
///    asserts the routed frames,
/// 3. require wall clock < 1 s (§17 M0, S2 acceptance).
pub fn patch_spike() -> i32 {
    if cfg!(windows) {
        println!("patch-spike: skipped on windows (dlopen protocol is unix-only at M0)");
        return 0;
    }
    let root = repo_root();
    let tip = root.join("tests/patch-tip");

    // The host is the fat build — prebuilt outside the save→frame clock.
    // Linux needs -rdynamic so dlsym(RTLD_DEFAULT, …) sees the host's
    // patchable symbols; macOS exports them by default.
    let host_rustflags: &[(&str, &str)] = if cfg!(target_os = "macos") {
        &[]
    } else {
        &[("RUSTFLAGS", "-C link-args=-rdynamic")]
    };
    if let Err(code) = run_cargo(
        &root,
        &["build", "-p", "dummy-host", "--quiet"],
        host_rustflags,
        false,
    ) {
        eprintln!("patch-spike: dummy-host build failed ({code})");
        return 1;
    }

    // "Save": the tip source changed. The clock starts here. M3's ThinLink
    // story is two generations against one running host: v1 (label/count)
    // and v2 (which also fills the host's stub thunk). The budget applies
    // per save — each generation's build + apply + first frame — and to the
    // whole gate.
    let t0 = std::time::Instant::now();

    let tip_target = tip.join("target");
    // option_env! values are not fingerprinted by cargo — drop the tip's
    // target dir (a tiny crate) so each generation's bodies really get
    // compiled in.
    if tip_target.exists() {
        std::fs::remove_dir_all(&tip_target).expect("clean tip target");
    }
    if let Err(code) = run_cargo(
        &tip,
        &["build", "--quiet"],
        &[("LIG_TIP_LABEL", "v1"), ("LIG_TIP_COUNT", "42")],
        false,
    ) {
        eprintln!("patch-spike: tip v1 build failed ({code})");
        return 1;
    }
    // Each generation gets its own artifact, staged OUTSIDE the tip's target
    // dir (the v2 build wipes it): the second build overwrites the tip's
    // output path, and the host must load v1 and v2 side by side.
    let stage_dir = root.join("target/lig-patch-spike");
    std::fs::create_dir_all(&stage_dir).expect("stage dir");
    let dylib1 = {
        let built = tip_dylib(&tip_target);
        let staged = stage_dir.join("gen1.dylib");
        std::fs::copy(&built, &staged).expect("stage generation 1 dylib");
        staged
    };

    let t_save2 = std::time::Instant::now();
    if tip_target.exists() {
        std::fs::remove_dir_all(&tip_target).expect("clean tip target for v2");
    }
    if let Err(code) = run_cargo(
        &tip,
        &["build", "--quiet"],
        &[
            ("LIG_TIP_LABEL", "v2"),
            ("LIG_TIP_COUNT", "43"),
            ("LIG_TIP_EXTRA", "7"),
        ],
        false,
    ) {
        eprintln!("patch-spike: tip v2 build failed ({code})");
        return 1;
    }
    let dylib2 = {
        let built = tip_dylib(&tip_target);
        let staged = tip_target.join("gen2.dylib");
        std::fs::copy(&built, &staged).expect("stage generation 2 dylib");
        staged
    };
    let save2 = t_save2.elapsed();

    let host_bin = root.join("target/debug/dummy-host");
    let out = std::process::Command::new(&host_bin)
        .env("LIG_PATCH_DYLIB", &dylib1)
        .env("LIG_PATCH_DYLIB2", &dylib2)
        .output()
        .unwrap_or_else(|e| panic!("spawn {}: {e}", host_bin.display()));
    let total = t0.elapsed();
    if !out.status.success() {
        eprintln!("patch-spike: dummy-host failed:");
        eprint!("{}", String::from_utf8_lossy(&out.stderr));
        eprint!("{}", String::from_utf8_lossy(&out.stdout));
        return 1;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let field = |key: &str| -> Option<u128> {
        stdout.lines().find_map(|l| {
            l.strip_prefix(key)?
                .strip_prefix('=')
                .and_then(|v| v.trim().parse::<u128>().ok())
        })
    };
    let Some(apply_ns) = field("apply_ns") else {
        eprintln!("patch-spike: dummy-host did not report apply_ns:\n{stdout}");
        return 1;
    };
    let Some(frame_ns) = field("frame_ns") else {
        eprintln!("patch-spike: dummy-host did not report frame_ns:\n{stdout}");
        return 1;
    };
    if field("apply2_ns").is_none() {
        eprintln!("patch-spike: dummy-host did not report apply2_ns:\n{stdout}");
        return 1;
    }
    if field("frame2_ns").is_none() {
        eprintln!("patch-spike: dummy-host did not report frame2_ns:\n{stdout}");
        return 1;
    }
    if !stdout.contains("host_result=PASS") {
        eprintln!("patch-spike: dummy-host did not pass:\n{stdout}");
        return 1;
    }

    let budget = std::time::Duration::from_secs(1);
    let total_ms = total.as_millis();
    // Per-save budget: each generation's build → apply → first frame.
    let ok = total < budget && save2 < budget;
    println!(
        "patch-spike: two generations, {} ms total (gen1 apply {} µs, frame {} µs; gen2 save→apply {} ms) — {}",
        total_ms,
        apply_ns / 1_000,
        frame_ns / 1_000,
        save2.as_millis(),
        if ok {
            "PASS (< 1 s per save, < 1 s total)"
        } else {
            "FAIL"
        }
    );
    i32::from(!ok)
}

// ---------------------------------------------------------------------------
// D-9 incremental gate (M3): digest-gated re-emission + cycle recovery
// ---------------------------------------------------------------------------

/// The D-9 gate (`lig incremental`): drives a session through scripted edits
/// over a temp dir and proves, end to end, that
///
/// 1. the first build writes every unit,
/// 2. an unedited rebuild **keeps** the artifacts byte-for-byte (same-session
///    memo reuse AND cross-session state gating),
/// 3. one edit re-emits exactly the edited unit,
/// 4. reverting the edit re-emits byte-identical output (D-8 across passes),
/// 5. a cyclic component source denies with LIG-E020 — never a hang — and a
///    denied pass never touches `dist` (BR-1 for watch).
pub fn incremental() -> i32 {
    let tmp = std::env::temp_dir().join(format!("lig-incremental-gate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let src = tmp.join("src");
    let dist = tmp.join("dist");
    if std::fs::create_dir_all(&src).is_err() {
        eprintln!("incremental: cannot create the temp workspace");
        return 1;
    }
    let counter_path = src.join("counter.tsx");
    std::fs::write(&counter_path, lig_parse_solid::tests_support::COUNTER_TSX)
        .expect("seed counter source");

    let fail = |msg: String| {
        eprintln!("incremental: {msg}");
        let _ = std::fs::remove_dir_all(&tmp);
        1
    };
    let dist_rs = || dist.join("counter.rs");
    let file_hash = |p: &std::path::Path| -> Option<u64> {
        std::fs::read(p).ok().map(|b| lig_core::hash::fnv1a64(&b))
    };

    let profile = lig_admission::Profile::Layout;

    // Pass 1 — first build writes.
    let mut session = lig_incremental::Session::new();
    session.set_source(&counter_path, counter_text());
    session.compile(profile);
    if let Err(e) =
        lig_incremental::emit::emit_units(&dist, session.units().as_slice(), "taffy", false)
    {
        return fail(format!("pass 1 emit failed: {e}"));
    }
    let Some(original_hash) = file_hash(&dist_rs()) else {
        return fail("pass 1 did not write counter.rs".to_string());
    };

    // Pass 2 — same session, no edits: full memo reuse, everything kept.
    let r2 = session.compile(profile);
    let e2 = lig_incremental::emit::emit_units(&dist, session.units().as_slice(), "taffy", false);
    match e2 {
        Ok(rep) if rep.written.is_empty() && rep.kept.len() == 1 => {}
        Ok(rep) => return fail(format!("pass 2 expected 1 kept / 0 written, got {rep:?}")),
        Err(e) => return fail(format!("pass 2 emit failed: {e}")),
    }
    if !r2.all_reused() {
        return fail("pass 2 must serve every query from memos".to_string());
    }
    if file_hash(&dist_rs()) != Some(original_hash) {
        return fail("pass 2 touched a byte-identical artifact".to_string());
    }

    // Pass 3 — a fresh session (cold memos, warm build-state): still all kept.
    let mut session = lig_incremental::Session::new();
    session.set_source(&counter_path, counter_text());
    session.compile(profile);
    match lig_incremental::emit::emit_units(&dist, session.units().as_slice(), "taffy", false) {
        Ok(rep) if rep.written.is_empty() && rep.kept.len() == 1 => {}
        Ok(rep) => return fail(format!("pass 3 expected state-gated keep, got {rep:?}")),
        Err(e) => return fail(format!("pass 3 emit failed: {e}")),
    }

    // Pass 4 — one edit re-emits exactly the edited unit, with different bytes.
    let edited = counter_text().replace("Add", "Add2");
    session.set_source(&counter_path, edited.clone());
    let r4 = session.compile(profile);
    if r4.recompiled.len() != 1 {
        return fail("pass 4 must recompile exactly the edited unit".to_string());
    }
    match lig_incremental::emit::emit_units(&dist, session.units().as_slice(), "taffy", false) {
        Ok(rep) if rep.written.len() == 1 && rep.kept.is_empty() => {}
        Ok(rep) => return fail(format!("pass 4 expected 1 written, got {rep:?}")),
        Err(e) => return fail(format!("pass 4 emit failed: {e}")),
    }
    let Some(edited_hash) = file_hash(&dist_rs()) else {
        return fail("pass 4 lost counter.rs".to_string());
    };
    if edited_hash == original_hash {
        return fail("pass 4 emitted identical bytes for an edited source".to_string());
    }

    // Pass 5 — revert: re-emits byte-identical output (D-8 across passes).
    session.set_source(&counter_path, counter_text());
    session.compile(profile);
    match lig_incremental::emit::emit_units(&dist, session.units().as_slice(), "taffy", false) {
        Ok(rep) if rep.written.len() == 1 => {}
        Ok(rep) => return fail(format!("pass 5 expected a re-emit, got {rep:?}")),
        Err(e) => return fail(format!("pass 5 emit failed: {e}")),
    }
    if file_hash(&dist_rs()) != Some(original_hash) {
        return fail("pass 5 reverted output is not byte-identical (D-8)".to_string());
    }

    // Pass 6 — cycle recovery: LIG-E020 denies, dist untouched (BR-1).
    let cycle_path = src.join("cycle.tsx");
    session.set_source(&cycle_path, cycle_text());
    session.compile(profile);
    let Some(unit) = session.unit(&cycle_path) else {
        return fail("pass 6 lost the cycle unit".to_string());
    };
    if unit.admitted {
        return fail("pass 6 admitted a cyclic component import".to_string());
    }
    if !unit.diagnostics.iter().any(|d| d.code == "LIG-E020") {
        return fail("pass 6 did not surface LIG-E020".to_string());
    }
    if file_hash(&dist_rs()) != Some(original_hash) {
        return fail("pass 6 let a denied pass touch dist (BR-1)".to_string());
    }

    let _ = std::fs::remove_dir_all(&tmp);
    println!(
        "incremental: digest-gated re-emit green (6 passes: write, memo-keep, state-keep, edit, revert, LIG-E020 deny)"
    );
    0
}

/// The counter fixture text (inlined so the gate never depends on CWD).
fn counter_text() -> &'static str {
    lig_parse_solid::tests_support::COUNTER_TSX
}

/// The cyclic-component source (LIG-E020 recovery case).
fn cycle_text() -> &'static str {
    "export function A() {\n  return <B />;\n}\n\nexport function B() {\n  return <A />;\n}\n"
}

// ---------------------------------------------------------------------------
// D-7 catalog machinery + NFR-5/V.7 admission gates
// ---------------------------------------------------------------------------

/// Regenerate `crates/lig-core/src/error_code/generated.rs` from
/// `docs/errors/*.md` (`lig errors-gen`; Svelte's catalog machinery, §2.3).
pub fn errors_gen() -> i32 {
    let root = repo_root();
    let dir = root.join("docs/errors");
    let mut rows: Vec<(String, String, bool, String)> = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read docs/errors: {e}"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    entries.sort();
    for path in &entries {
        let text = std::fs::read_to_string(path).expect("read error doc");
        // Path components fall back to the display path instead of panicking
        // on stemless names (`.md`, filesystem roots) — a gate must fail with
        // a message, never an unwind (M16 debt: the weak-error unwraps).
        let stem = path.file_stem().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let title = text
            .lines()
            .find_map(|l| l.strip_prefix(&format!("# {stem} — ")))
            .map_or_else(|| stem.clone(), str::to_string);
        let status = text
            .lines()
            .find_map(|l| l.strip_prefix("Status: "))
            .unwrap_or("active")
            .trim();
        let (reserved, stage) = if let Some(rest) = status.strip_prefix("active") {
            let _ = rest;
            (false, "active".to_string())
        } else {
            // e.g. `reserved (M2)` / `deny (M1)` / `lint (M1)`
            let stage = status
                .split(['(', ' ', ')'])
                .find(|t| t.starts_with('M'))
                .unwrap_or("M1")
                .to_string();
            (true, stage)
        };
        rows.push((stem, title, reserved, stage));
    }
    let mut out = String::from(
        "// @generated by `lig errors-gen` from docs/errors/*.md — do not edit.\n\nuse super::ErrorCode;\n\npub static REGISTRY: &[ErrorCode] = &[\n",
    );
    for (code, title, reserved, stage) in &rows {
        write!(
            out,
            "    ErrorCode {{\n        code: \"{code}\",\n        title: \"{title}\",\n        reserved: {reserved},\n        stage: \"{stage}\",\n    }},\n"
        )
        .expect("write to String is infallible");
    }
    out.push_str("];\n");
    let path = root.join("crates/lig-core/src/error_code/generated.rs");
    std::fs::write(&path, out).expect("write generated.rs");

    // M3: the embedded catalog — `lig explain` serves these without a repo
    // checkout (the `lig doctor` hardening).
    let mut cat = String::from(
        "// @generated by `lig errors-gen` from docs/errors/*.md — do not edit.\n\n//! Embedded catalog Markdown bodies (M3): shipped inside the binary so\n//! `lig explain` works far from the workspace.\n\n// Raw-string fences and literal Markdown bodies are generator-owned;\n// rustfmt must not reindent them.\n#[rustfmt::skip]\npub static CATALOG: &[(&str, &str)] = &[\n",
    );
    let mut embedded = 0usize;
    for path_md in &entries {
        let text = std::fs::read_to_string(path_md).expect("read error doc");
        let stem = path_md.file_stem().map_or_else(
            || path_md.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        // Raw-string delimiters adapt to the content: a literal quote
        // followed by the fence inside the Markdown would terminate the raw
        // string, so grow the fence (0 hashes first — clippy wants minimal).
        let mut hashes = 0usize;
        while text.contains(&format!("\"{}", "#".repeat(hashes))) {
            hashes += 1;
        }
        let fence = "#".repeat(hashes);
        let _ = writeln!(cat, "    (\"{stem}\", r{fence}\"{text}\"{fence}),");
        embedded += 1;
    }
    cat.push_str("];\n");
    let cat_path = root.join("crates/lig-core/src/error_code/catalog_md.rs");
    std::fs::write(&cat_path, cat).expect("write catalog_md.rs");
    println!(
        "errors-gen: {} codes ({} embedded) → {}, {}",
        rows.len(),
        embedded,
        path.display(),
        cat_path.display()
    );
    0
}

/// NFR-5 admission fixture harness (`lig fixtures admission`, §13): every
/// `fixtures/admission/**` case must produce exactly its expected
/// diagnostics, and every Deny must render per NFR-5 (file:line:col,
/// ≤120-char excerpt, code, suggestion/URL).
pub fn admission() -> i32 {
    let root = repo_root();
    let base = root.join("fixtures/admission");
    let mut failures = 0usize;
    let mut cases = 0usize;
    let mut dirs: Vec<_> = std::fs::read_dir(&base)
        .unwrap_or_else(|e| panic!("read {}: {e}", base.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for rule_dir in dirs {
        for tier in ["allow", "deny", "lint"] {
            let case = rule_dir.join(tier);
            if !case.exists() {
                continue;
            }
            cases += 1;
            if let Err(e) = run_admission_case(&case) {
                eprintln!("  FAIL {}: {e}", case.display());
                failures += 1;
            }
        }
    }
    if failures == 0 {
        println!("admission: {cases} case(s) green (NFR-5)");
        0
    } else {
        eprintln!("admission: {failures}/{cases} case(s) failed");
        1
    }
}

fn run_admission_case(case_dir: &std::path::Path) -> Result<(), String> {
    let input = case_dir.join("input.solid.tsx");
    let source =
        std::fs::read_to_string(&input).map_err(|e| format!("read {}: {e}", input.display()))?;
    let expected_path = case_dir.join("expected.json");
    if !expected_path.exists() {
        // allow-tier cases expect clean admission.
        return expect_clean(case_dir, &source);
    }
    let expected: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&expected_path).expect("read expected.json"))
            .map_err(|e| format!("parse {}: {e}", expected_path.display()))?;
    let profile = match expected["profile"].as_str().unwrap_or("layout") {
        "web" => lig_admission::Profile::Web,
        _ => lig_admission::Profile::Layout,
    };
    // Lower (lowering diagnostics participate in the report), then admit.
    let diags: Vec<(String, usize)> =
        match lig_parse_solid::lower_module(&source, "input.solid.tsx") {
            Ok(m) => lig_admission::admit(&m, profile)
                .diagnostics
                .iter()
                .map(|d| (d.code.clone(), line_of(&source, d.span.start)))
                .collect(),
            Err(errs) => errs
                .iter()
                .map(|d| (d.code.clone(), line_of(&source, d.span.start)))
                .collect(),
        };
    let wants = expected["expected"]
        .as_array()
        .ok_or("expected.json: `expected` must be an array")?;
    for want in wants {
        let code = want["code"].as_str().ok_or("expected entry needs code")?;
        let line = want["line"].as_u64().unwrap_or(0) as usize;
        if !diags.iter().any(|(c, l)| c == code && *l == line) {
            return Err(format!(
                "{}: expected {code} at line {line}; got {diags:?}",
                case_dir.display()
            ));
        }
    }
    if diags.len() != wants.len() {
        return Err(format!(
            "{}: expected {} diagnostics, got {diags:?}",
            case_dir.display(),
            wants.len()
        ));
    }
    // NFR-5 rendering contract for deny/lint cases: file:line:col header,
    // code, ≤120-char offending excerpt, and the catalog URL.
    let name = case_dir.file_name().map_or_else(
        || case_dir.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let rendered = match lig_parse_solid::lower_module(&source, &name) {
        Ok(m) => {
            let rep = lig_admission::admit(&m, profile);
            rep.diagnostics
                .first()
                .map(|d| lig_core::diagnostic::render(&name, &source, d))
        }
        Err(errs) => errs
            .first()
            .map(|d| lig_core::diagnostic::render(&name, &source, d)),
    };
    if let Some(r) = rendered {
        if !r.contains(&name) || !r.contains("ligature.dev/e/") {
            return Err(format!(
                "{name}: NFR-5 render missing file header or URL:\n{r}"
            ));
        }
        if r.lines().any(|l| l.chars().count() > 160) {
            return Err(format!("{name}: NFR-5 render exceeds width budget"));
        }
    }
    Ok(())
}

fn expect_clean(case_dir: &std::path::Path, source: &str) -> Result<(), String> {
    let name = case_dir.parent().and_then(|p| p.file_name()).map_or_else(
        || case_dir.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    match lig_parse_solid::lower_module(source, "input.solid.tsx") {
        Ok(m) => {
            let rep = lig_admission::admit(&m, lig_admission::Profile::Layout);
            if rep.is_admitted() {
                Ok(())
            } else {
                Err(format!(
                    "{name}/allow: expected clean admission, got {:?}",
                    rep.diagnostics
                ))
            }
        }
        Err(errs) => Err(format!("{name}/allow: lowering failed: {errs:?}")),
    }
}

fn line_of(source: &str, offset: u32) -> usize {
    source
        .bytes()
        .take(offset as usize)
        .filter(|b| *b == b'\n')
        .count()
        + 1
}

/// V.7 rule-table ↔ fixture 1:1 gate (`lig fixtures rules-sync`).
pub fn rules_sync() -> i32 {
    let root = repo_root();
    match lig_admission::rules::load(&root).and_then(|rules| {
        println!("rules-sync: {} rule rows", rules.len());
        lig_admission::rules::check_fixture_sync(&root, &rules)
    }) {
        Ok(()) => {
            println!("rules-sync: 1:1 with fixtures/admission ✓");
            0
        }
        Err(e) => {
            eprintln!("rules-sync: {e}");
            1
        }
    }
}
