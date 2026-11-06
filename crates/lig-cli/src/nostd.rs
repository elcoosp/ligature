//! §10 no_std proof gate (`lig no-std`): the runtime substrate's std-free
//! contract, proven — not claimed.
//!
//! Three legs:
//!
//! 1. **Structural contract** — `lig-runtime` declares `#![no_std]` +
//!    `#![forbid(unsafe_code)]`, carries zero `[dependencies]` (dev
//!    dependencies excepted — test harnesses run on the host), and touches
//!    `std` only under `#[cfg(test)]`.
//! 2. **Bare-metal builds** — the crate compiles for real no_std targets
//!    (`thumbv7em-none-eabihf`, `aarch64-unknown-none-softfloat`), where
//!    neither std nor a host allocator exists to hide behind.
//! 3. **Generated code** — a fresh counter unit (the D-8-pinned workload)
//!    compiles inside a `#![no_std]` scratch crate against `lig-runtime` for
//!    the thumb target: generated code is host-agnostic too, not just the
//!    runtime it links.
//!
//! Missing targets fail with the one-line fix (`rustup target add …`); CI
//! installs them in the dedicated `no-std` job.

use crate::gates::{counter_module, repo_root, run_cargo};
use std::path::Path;

/// The no_std targets the proof builds for: a Cortex-M4F (the canonical
/// embedded UI target, hard-float) and a 64-bit soft-float bare-metal core.
const TARGETS: &[&str] = &["thumbv7em-none-eabihf", "aarch64-unknown-none-softfloat"];

/// The target that additionally proves a generated unit compiles no_std.
const GENERATED_PROOF_TARGET: &str = "thumbv7em-none-eabihf";

/// Structural contract over the committed crate: zero runtime dependencies,
/// `#![no_std]`, `#![forbid(unsafe_code)]`, and `std` only under `#[cfg(test)]`.
fn check_structural(root: &Path) -> Result<(), String> {
    let manifest = root.join("crates/lig-runtime/Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|e| format!("read {}: {e}", manifest.display()))?;
    let value: toml::Value =
        toml::from_str(&text).map_err(|e| format!("parse {}: {e}", manifest.display()))?;
    let deps = value.get("dependencies").and_then(toml::Value::as_table);
    match deps {
        None => {}
        Some(t) if t.is_empty() => {}
        Some(t) => {
            return Err(format!(
                "lig-runtime declares [dependencies] ({} row(s)) — the no_std \
                 substrate must stay dependency-free (ADR-012 discipline)",
                t.len()
            ));
        }
    }

    let lib = root.join("crates/lig-runtime/src/lib.rs");
    let src = std::fs::read_to_string(&lib).map_err(|e| format!("read {}: {e}", lib.display()))?;
    if !src.contains("#![no_std]") {
        return Err("lig-runtime is missing #![no_std]".to_string());
    }
    if !src.contains("#![forbid(unsafe_code)]") {
        return Err("lig-runtime is missing #![forbid(unsafe_code)]".to_string());
    }
    // `extern crate std` must sit behind `#[cfg(test)]` — host test
    // harnesses only (the crate comment states this; the gate enforces it).
    let mut prev_test_cfg = false;
    for line in src.lines() {
        let trimmed = line.trim();
        if trimmed == "#[cfg(test)]" {
            prev_test_cfg = true;
            continue;
        }
        if trimmed.starts_with("extern crate std") && !prev_test_cfg {
            return Err("lig-runtime pulls in std outside #[cfg(test)] — not no_std".to_string());
        }
        if !trimmed.is_empty() && !trimmed.starts_with("//") {
            prev_test_cfg = false;
        }
    }
    Ok(())
}

/// Whether a rustup target is installed (best-effort probe; a failing
/// `rustup` invocation reports the target as missing — the build step's
/// error is the actionable one).
fn target_installed(target: &str) -> bool {
    let out = std::process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout.lines().any(|l| l.trim() == target)
        }
        _ => false,
    }
}

/// The scratch crate that proves a generated unit is no_std: a standalone
/// `#![no_std]` lib (in the temp dir, outside this workspace) that
/// `include!`s a freshly generated counter unit and links `lig-runtime` by
/// absolute path.
fn generated_unit_proof(root: &Path, target: &str) -> Result<(), String> {
    let scratch = std::env::temp_dir().join(format!("lig-no-std-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(scratch.join("src"))
        .map_err(|e| format!("create {}: {e}", scratch.display()))?;

    // Fresh generation — the same unit `lig reproducibility` pins byte-exact.
    let module = counter_module();
    let unit = lig_codegen::generate_module(&module)
        .map_err(|e| format!("codegen for the no_std proof failed: {e}"))?;
    let gen_path = scratch.join("src/counter_unit.rs");
    std::fs::write(&gen_path, &unit.text)
        .map_err(|e| format!("write {}: {e}", gen_path.display()))?;

    let manifest = format!(
        "[package]\nname = \"lig-no-std-proof\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
         publish = false\n\n[workspace]\n\n[dependencies]\nlig-runtime = {{ path = '{}' }}\n",
        root.join("crates/lig-runtime").display()
    );
    std::fs::write(scratch.join("Cargo.toml"), manifest)
        .map_err(|e| format!("write scratch manifest: {e}"))?;

    // The include! path is a Rust string literal: escape `\\` and `"` —
    // temp-dir paths are plain, the escape keeps exotic TMPDIRs honest.
    let gen_path_lit = gen_path
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let lib_src = format!(
        "//! @generated by `lig no-std` — scratch proof crate, not committed.\n\
         #![no_std]\nextern crate alloc;\n\ninclude!(\"{gen_path_lit}\");\n"
    );
    std::fs::write(scratch.join("src/lib.rs"), lib_src)
        .map_err(|e| format!("write scratch lib.rs: {e}"))?;

    let build = run_cargo(
        &scratch,
        &[
            "build",
            "--manifest-path",
            &scratch.join("Cargo.toml").to_string_lossy(),
            "--target",
            target,
            "--quiet",
        ],
        &[],
        true,
    );
    let result = match build {
        Ok(_) => Ok(()),
        Err(code) => Err(format!(
            "generated unit does not compile no_std for {target} (cargo exit {code})"
        )),
    };
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

/// The no_std proof gate (`lig no-std`).
///
/// Exit codes: 0 = proof green, 1 = broken contract, missing target, or a
/// failed bare-metal build.
pub fn no_std_proof() -> i32 {
    let root = repo_root();

    // 1. The structural contract (fast, no toolchain work).
    if let Err(e) = check_structural(&root) {
        eprintln!("no-std: {e}");
        return 1;
    }
    println!(
        "no-std: lig-runtime — zero [dependencies], #![no_std], forbid(unsafe_code), std test-only ✓"
    );

    // 2. Bare-metal builds. A missing target is a hard failure with the fix:
    // the proof must not silently pass where it did not run.
    for target in TARGETS {
        if !target_installed(target) {
            eprintln!(
                "no-std: target {target} is not installed — run `rustup target add {}`",
                TARGETS.join(" ")
            );
            return 1;
        }
        match run_cargo(
            &root,
            &["build", "-p", "lig-runtime", "--target", target, "--quiet"],
            &[],
            true,
        ) {
            Ok(_) => println!("no-std: {target} — lig-runtime builds ✓"),
            Err(code) => {
                eprintln!("no-std: {target} — lig-runtime build failed (cargo exit {code})");
                return 1;
            }
        }
    }

    // 3. Generated code is host-agnostic: a fresh counter unit compiles
    // inside a #![no_std] scratch crate for the thumb target.
    match generated_unit_proof(&root, GENERATED_PROOF_TARGET) {
        Ok(()) => println!(
            "no-std: {GENERATED_PROOF_TARGET} — generated counter unit builds in a #![no_std] crate ✓"
        ),
        Err(e) => {
            eprintln!("no-std: {e}");
            return 1;
        }
    }

    println!(
        "no-std: proof green (§10 — runtime + generated code are std-free across {} targets)",
        TARGETS.len()
    );
    0
}

/// The installed proof targets, for `lig doctor`'s environment report.
/// Returns `(installed, total)`.
pub fn installed_targets() -> (usize, usize) {
    (
        TARGETS.iter().filter(|t| target_installed(t)).count(),
        TARGETS.len(),
    )
}

/// The target list for doctor's hint line.
pub fn target_names() -> &'static [&'static str] {
    TARGETS
}
