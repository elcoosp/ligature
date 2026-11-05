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
