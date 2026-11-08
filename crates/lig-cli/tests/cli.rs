//! `lig check` / `lig build` / `lig explain` end-to-end (§12.1, M1 exit
//! criteria): exit codes 0 = clean, 1 = denied; fail-closed build (BR-1)
//! emits Rust + IR + chained source maps + manifest, and `explain` serves
//! the generated catalog (D-7).

use std::process::Command;

fn lig() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lig"))
}

fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn check_admits_the_counter_and_reports_clean() {
    let src = repo_root().join("fixtures/conformance/counter/input.solid.tsx");
    let out = lig()
        .args(["check", &src.to_string_lossy()])
        .output()
        .expect("spawn lig check");
    assert!(out.status.success(), "exit 0 on admitted source");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("admitted"), "clean summary: {stdout}");
}

#[test]
fn check_denies_a_deny_tier_construct_with_exit_one() {
    let src = repo_root().join("fixtures/admission/e114-callback-call/deny/input.solid.tsx");
    let out = lig()
        .args(["check", &src.to_string_lossy()])
        .output()
        .expect("spawn lig check");
    assert_eq!(out.status.code(), Some(1), "exit 1 = denied");
    let stderr = String::from_utf8_lossy(&out.stderr);
    // NFR-5 rendering: code frame with the stable code and catalog URL.
    assert!(stderr.contains("LIG-E114"), "stable code present: {stderr}");
    assert!(
        stderr.contains("ligature.dev/e/"),
        "catalog URL present: {stderr}"
    );
}

#[test]
fn build_emits_rust_ir_maps_and_manifest_fail_closed() {
    let src = repo_root().join("fixtures/conformance/counter/input.solid.tsx");
    let dist = std::env::temp_dir().join("lig-cli-build-test");
    let _ = std::fs::remove_dir_all(&dist);
    let out = lig()
        .args(["build", &src.to_string_lossy(), "--out"])
        .arg(&dist)
        .output()
        .expect("spawn lig build");
    assert!(out.status.success(), "build succeeds: {out:?}");
    for f in [
        "counter.rs",
        "ir/counter.json",
        "maps/counter.map.json",
        "manifest.toml",
    ] {
        let p = dist.join(f);
        assert!(p.exists(), "emitted {f}");
        let meta = std::fs::metadata(&p).expect("metadata");
        assert!(meta.len() > 0, "{f} is non-empty");
    }
    // The chained map is standard VLQ JSON: sources resolve to the fixture
    // and the IR node names ride in `names` (§9.4).
    let map = std::fs::read_to_string(dist.join("maps/counter.map.json")).expect("read map");
    assert!(map.contains("\"mappings\""), "VLQ mappings present");
    assert!(map.contains("\"names\""), "IR node names present");
    let _ = std::fs::remove_dir_all(&dist);
}

#[test]
fn explain_serves_the_generated_catalog() {
    // The CLI resolves docs/errors relative to the workspace root.
    let out = lig()
        .current_dir(repo_root())
        .args(["explain", "LIG-E114"])
        .output()
        .expect("spawn lig explain");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("LIG-E114"), "doc body echoes the code");
    assert!(
        stdout.contains("ligature.dev/e/LIG-E114"),
        "catalog URL: {stdout}"
    );
}

#[test]
fn fixtures_verify_gate_runs_both_frontend_legs() {
    let out = lig()
        .current_dir(repo_root())
        .args(["fixtures", "verify"])
        .output()
        .expect("spawn lig fixtures verify");
    assert!(out.status.success(), "verify green: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("match the committed canonical IR"),
        "summary present: {stdout}"
    );
    // M2: every fixture must have both legs verified.
    assert!(stdout.contains("vue leg OK"), "vue legs verified: {stdout}");
}

#[test]
fn fixtures_admission_gate_is_green() {
    let out = lig()
        .current_dir(repo_root())
        .args(["fixtures", "admission"])
        .output()
        .expect("spawn lig fixtures admission");
    assert!(out.status.success(), "admission green: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("22 case(s) green"), "NFR-5 cases: {stdout}");
}

#[test]
fn rules_sync_and_reproducibility_gates_are_green() {
    let runs: [&[&str]; 2] = [&["fixtures", "rules-sync"], &["reproducibility"]];
    for args in runs {
        let out = lig()
            .current_dir(repo_root())
            .args(args)
            .output()
            .unwrap_or_else(|e| panic!("spawn lig {args:?}: {e}"));
        assert!(out.status.success(), "lig {args:?} green: {out:?}");
    }
}

#[test]
fn digest_gated_build_keeps_unchanged_units() {
    let src = repo_root().join("fixtures/conformance/counter/input.solid.tsx");
    let dist = std::env::temp_dir().join(format!("lig-cli-d9-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dist);
    let out = lig()
        .args(["build", &src.to_string_lossy(), "--out"])
        .arg(&dist)
        .output()
        .expect("spawn lig build #1");
    assert!(out.status.success(), "build #1 succeeds: {out:?}");
    let stdout1 = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout1.contains("1 re-emitted"),
        "first build writes: {stdout1}"
    );
    let rs = dist.join("counter.rs");
    let bytes1 = std::fs::read(&rs).expect("counter.rs exists");
    assert!(dist.join("build-state.json").exists(), "state file emitted");

    let out = lig()
        .args(["build", &src.to_string_lossy(), "--out"])
        .arg(&dist)
        .output()
        .expect("spawn lig build #2");
    assert!(out.status.success(), "build #2 succeeds: {out:?}");
    let stdout2 = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout2.contains("1 kept, 0 re-emitted"),
        "unchanged rebuild keeps (D-9): {stdout2}"
    );
    assert_eq!(
        std::fs::read(&rs).expect("counter.rs still there"),
        bytes1,
        "kept artifact is byte-identical"
    );
    let _ = std::fs::remove_dir_all(&dist);
}

#[test]
fn build_denies_cyclic_components_with_lig_e020_and_writes_nothing() {
    let dir = std::env::temp_dir().join(format!("lig-cli-e020-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp src dir");
    let src = dir.join("cycle.tsx");
    std::fs::write(
        &src,
        "export function A() {\n  return <B />;\n}\n\nexport function B() {\n  return <A />;\n}\n",
    )
    .expect("seed cycle");
    let dist = dir.join("dist");
    let out = lig()
        .args(["build", &src.to_string_lossy(), "--out"])
        .arg(&dist)
        .output()
        .expect("spawn lig build on a cycle");
    assert_eq!(out.status.code(), Some(1), "exit 1 = denied");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("LIG-E020"), "cycle code present: {stderr}");
    assert!(stderr.contains("BR-1"), "fail-closed note: {stderr}");
    assert!(!dist.exists(), "zero output written on denial");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn watch_once_runs_a_digest_gated_pass() {
    let src = repo_root().join("fixtures/conformance/counter/input.solid.tsx");
    let dist = std::env::temp_dir().join(format!("lig-cli-watch-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dist);
    let out = lig()
        .args([
            "watch",
            &src.to_string_lossy(),
            "--out",
            &dist.to_string_lossy(),
            "--once",
        ])
        .output()
        .expect("spawn lig watch --once");
    assert!(out.status.success(), "watch --once is green: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("lig watch"), "watch summary: {stdout}");
    assert!(dist.join("counter.rs").exists(), "watch emitted the unit");
    let _ = std::fs::remove_dir_all(&dist);
}

#[test]
fn incremental_gate_is_green() {
    let out = lig()
        .current_dir(repo_root())
        .args(["incremental"])
        .output()
        .expect("spawn lig incremental");
    assert!(out.status.success(), "incremental gate: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("digest-gated re-emit green"), "{stdout}");
}

#[test]
fn doctor_reports_all_checks() {
    let out = lig().args(["doctor"]).output().expect("spawn lig doctor");
    assert!(out.status.success(), "doctor green: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("error catalog embedded"), "{stdout}");
    // M4: the budget ledger is an environment check too.
    assert!(stdout.contains("budget ledger"), "{stdout}");
    assert!(stdout.contains("no_std proof targets"), "{stdout}");
    assert!(stdout.contains("all checks pass"), "{stdout}");
}

#[test]
fn explain_serves_the_embedded_catalog_far_from_the_repo() {
    // Run from the temp dir: no docs/errors nearby, no ../docs — the M3
    // embedded catalog must serve the code.
    let out = lig()
        .current_dir(std::env::temp_dir())
        .args(["explain", "LIG-E114"])
        .output()
        .expect("spawn lig explain from /tmp");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("LIG-E114"),
        "doc body echoes the code: {stdout}"
    );
    assert!(
        stdout.contains("ligature.dev/e/LIG-E114"),
        "catalog URL: {stdout}"
    );
}

#[test]
fn budgets_gate_reports_the_ledger_table() {
    // Profile-independent assertions only: a dev run enforces the bytes row
    // and reports the time rows; the release run in CI enforces all of them.
    let out = lig()
        .current_dir(repo_root())
        .args(["budgets"])
        .output()
        .expect("spawn lig budgets");
    assert!(out.status.success(), "budgets gate green: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("gen_counter_bytes"), "bytes row: {stdout}");
    assert!(stdout.contains("7385"), "D-8-pinned byte count: {stdout}");
    assert!(
        stdout.contains("all 13 row(s) within budget"),
        "summary present: {stdout}"
    );
    assert!(
        stdout.contains("codegen_composed"),
        "M9 composed row: {stdout}"
    );
    assert!(
        stdout.contains("codegen_branch"),
        "M10 branch row: {stdout}"
    );
    assert!(stdout.contains("codegen_else"), "M11 else row: {stdout}");
}

#[test]
fn no_std_gate_proves_or_demands_targets() {
    // The gate is strict: a missing proof target is exit 1 with the fix. The
    // test asserts the right behavior in BOTH environments — with the
    // targets installed it proves the runtime + a generated unit compile
    // no_std; without them it demands `rustup target add`.
    let probe = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .expect("probe rustup targets");
    let installed = String::from_utf8_lossy(&probe.stdout);
    let targets_ready = installed
        .lines()
        .any(|l| l.trim() == "thumbv7em-none-eabihf")
        && installed
            .lines()
            .any(|l| l.trim() == "aarch64-unknown-none-softfloat");

    let out = lig().args(["no-std"]).output().expect("spawn lig no-std");
    if targets_ready {
        assert!(out.status.success(), "no-std proof green: {out:?}");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("proof green"), "{stdout}");
        assert!(
            stdout.contains("generated counter unit builds"),
            "generated-unit leg: {stdout}"
        );
    } else {
        assert_eq!(out.status.code(), Some(1), "missing targets are a failure");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("rustup target add"),
            "fix hint present: {stderr}"
        );
    }
}
