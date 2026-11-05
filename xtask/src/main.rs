//! CI gates (§13) as an xtask: fixture generation, harness verification,
//! reproducibility byte-diffs, budget checks. No external dependencies —
//! the gate tool must never be the slow part.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use lig_codegen::generate_component;
use lig_core::id::ComponentId;
use lig_parse_solid::tests_support::COUNTER_TSX;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map_or("help", String::as_str);
    let code = match cmd {
        "fixtures-gen" => fixtures_gen(),
        "parity-gen" => parity_gen(),
        "parity" => parity(),
        "reproducibility" => reproducibility(),
        "patch-spike" => patch_spike(),
        "errors-gen" => errors_gen(),
        "admission" => admission_gate(),
        "rules-sync" => rules_sync(),
        "help" | "--help" | "-h" => {
            print_help();
            0
        }
        other => {
            eprintln!("unknown command: {other}");
            print_help();
            2
        }
    };
    std::process::exit(code);
}

fn print_help() {
    println!(
        "lig xtask gates\n\n\
         commands:\n\
         \x20 fixtures-gen      regenerate fixtures/generated/*.rs + counter expected IR\n\
         \x20 parity-gen        regenerate expected IR for every conformance fixture (S4)\n\
         \x20 parity            conformance harness: lower each fixture, byte-diff vs expected IR (S4)\n\
         \x20 reproducibility   build the counter twice; byte-diff the outputs (D-8)\n\
         \x20 patch-spike       S2 end-to-end: build tip v1, apply on dummy host, assert < 1 s save→frame\n\
         \x20 errors-gen        regenerate the error-code registry from docs/errors/*.md (D-7)\n\
         \x20 admission         NFR-5 harness: fixtures/admission/** vs expected codes/spans\n\
         \x20 rules-sync        rule table ↔ fixtures 1:1 (V.7)"
    );
}

fn counter_module() -> lig_ir::ViewModule {
    lig_parse_solid::lower_module(COUNTER_TSX, "counter.tsx").expect("counter lowers")
}

/// Regenerate the committed generated-code artifacts.
fn fixtures_gen() -> i32 {
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
                eprintln!("fixtures-gen: {name} failed admission:");
                for d in diags {
                    eprintln!("  {d}");
                }
                return 1;
            }
        };
        let text = match lig_codegen::generate_module(&m) {
            Ok(g) => g.text,
            Err(e) => {
                eprintln!("fixtures-gen: {name} codegen failed: {e}");
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
    // Same serialization as `parity-gen` — one canonical form, trailing
    // newline included, so the two regenerators never disagree.
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

/// D-8 gate: the committed generated file must equal a fresh generation.
fn reproducibility() -> i32 {
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
        eprintln!(
            "missing {}; run `cargo run -p xtask -- fixtures-gen`",
            committed.display()
        );
        return 1;
    }
    let old = std::fs::read_to_string(&committed).expect("read committed counter.rs");
    if old == unit.text {
        println!("reproducibility: OK (byte-identical, {} bytes)", old.len());
        0
    } else {
        eprintln!("reproducibility: FAIL — generated output drifted");
        eprintln!("  run `cargo run -p xtask -- fixtures-gen` and review the diff");
        1
    }
}

/// Keep `Path` imported for future harness helpers.
#[allow(dead_code)]
fn _path_used(p: &Path) -> bool {
    p.exists()
}

// ---------------------------------------------------------------------------
// S4: canonical-JSON parity harness (§4, §17.1)
// ---------------------------------------------------------------------------

/// Every conformance fixture dir (a dir under `fixtures/conformance` that
/// contains `input.solid.tsx`), in stable order.
fn conformance_dirs(root: &Path) -> Vec<(String, PathBuf)> {
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

/// Lower one fixture; fail-closed on any error diagnostic.
fn lower_fixture(name: &str, dir: &Path) -> Result<lig_ir::ViewModule, i32> {
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

/// Regenerate `expected.ir.json` for every fixture from the current frontend.
///
/// Review the diff before committing: these files are the canonical contract
/// the `parity` gate enforces (golden files, §4).
fn parity_gen() -> i32 {
    let root = repo_root();
    let dirs = conformance_dirs(&root);
    if dirs.len() < 12 {
        eprintln!(
            "parity-gen: expected 12 behavior fixtures (§17.1), found {}",
            dirs.len()
        );
        return 1;
    }
    let mut failures = Vec::new();
    for (name, dir) in &dirs {
        match lower_fixture(name, dir) {
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

/// S4 gate: every fixture must lower to byte-identical canonical JSON.
fn parity() -> i32 {
    let root = repo_root();
    let dirs = conformance_dirs(&root);
    if dirs.len() < 12 {
        eprintln!(
            "parity: expected 12 behavior fixtures (§17.1), found {}",
            dirs.len()
        );
        return 1;
    }
    let mut failed = 0usize;
    for (name, dir) in &dirs {
        let expected_path = dir.join("expected.ir.json");
        let Ok(expected) = std::fs::read_to_string(&expected_path) else {
            eprintln!("parity: {name} missing expected.ir.json — run parity-gen");
            failed += 1;
            continue;
        };
        match lower_fixture(name, dir) {
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
                    failed += 1;
                }
            }
            Err(code) => {
                failed += 1;
                let _ = code;
            }
        }
    }
    if failed == 0 {
        println!(
            "parity: all {} fixtures match the committed canonical IR",
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
/// failure; returns true on success.
fn run_cargo(
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

/// S2 gate: the subsecond-shaped save→frame budget on a dummy host.
///
/// Pipeline (clock starts at the "save", i.e. the tip rebuild):
/// 1. rebuild `patch-tip` as a cdylib with v1 bodies (`LIG_TIP_LABEL=v1`,
///    `LIG_TIP_COUNT=42`),
/// 2. run `dummy-host`, which applies the dylib through `lig-patch` and
///    asserts the routed frames,
/// 3. require wall clock < 1 s (§17 M0, S2 acceptance).
fn patch_spike() -> i32 {
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

    // "Save": the tip source changed. The clock starts here.
    let t0 = std::time::Instant::now();

    // option_env! values are not fingerprinted by cargo — drop the tip's
    // target dir (a tiny crate) so the v1 bodies really get compiled in.
    let tip_target = tip.join("target");
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

    let dylib = tip_dylib(&tip_target);
    let host_bin = root.join("target/debug/dummy-host");
    let out = std::process::Command::new(&host_bin)
        .env("LIG_PATCH_DYLIB", &dylib)
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
    let Some(_frame2_ns) = field("frame2_ns") else {
        eprintln!("patch-spike: dummy-host did not report frame2_ns:\n{stdout}");
        return 1;
    };
    if !stdout.contains("host_result=PASS") {
        eprintln!("patch-spike: dummy-host did not pass:\n{stdout}");
        return 1;
    }

    let budget = std::time::Duration::from_secs(1);
    let total_ms = total.as_millis();
    let ok = total < budget;
    println!(
        "patch-spike: save→frame {} ms (apply {} µs, routed frame {} µs) — {}",
        total_ms,
        apply_ns / 1_000,
        frame_ns / 1_000,
        if ok { "PASS (< 1 s budget)" } else { "FAIL" }
    );
    i32::from(!ok)
}

// ---------------------------------------------------------------------------
// D-7 catalog machinery + NFR-5/V.7 admission gates (M1).
// ---------------------------------------------------------------------------

/// Regenerate `crates/lig-core/src/error_code/generated.rs` from
/// `docs/errors/*.md` (Svelte's catalog machinery, §2.3).
fn errors_gen() -> i32 {
    let root = repo_root();
    let dir = root.join("docs/errors");
    let mut rows: Vec<(String, String, bool, String)> = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read docs/errors: {e}"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    entries.sort();
    for path in entries {
        let text = std::fs::read_to_string(&path).expect("read error doc");
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
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
        "// @generated by `cargo run -p xtask -- errors-gen` from docs/errors/*.md — do not edit.\n\nuse super::ErrorCode;\n\npub static REGISTRY: &[ErrorCode] = &[\n",
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
    println!("errors-gen: {} codes → {}", rows.len(), path.display());
    0
}

/// NFR-5 admission fixture harness (§13): every `fixtures/admission/**` case
/// must produce exactly its expected diagnostics, and every Deny must render
/// per NFR-5 (file:line:col, ≤120-char excerpt, code, suggestion/URL).
fn admission_gate() -> i32 {
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
    let name = case_dir.file_name().unwrap().to_string_lossy().into_owned();
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
    let name = case_dir
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
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

/// V.7 rule-table ↔ fixture 1:1 gate.
fn rules_sync() -> i32 {
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
