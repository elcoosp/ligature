//! The `lig` CLI (§12.1): `check`, `build`, `watch`, `explain`, `resolve`,
//! `inspect`, `doctor`, plus the CI gates (`fixtures …`, `reproducibility`,
//! `patch-spike`, `incremental`, `errors-gen`, `budgets`, `no-std`). M4
//! surface — the §14.2 budget ledger gate and the §10 no_std proof join the
//! incremental session engine (`lig-incremental`): digest-gated re-emission
//! (D-9), watch mode, build-order cycle recovery (LIG-E020), and the
//! two-generation ThinLink patch gate. Exit codes: 0 = clean, 1 = denied,
//! 2 = internal error.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod budgets;
mod gates;
mod nostd;

use clap::{Parser, Subcommand, ValueEnum};
use lig_admission::Profile;
use lig_core::diagnostic::render;
use lig_incremental::Session;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "lig",
    version,
    about = "Ligature — AOT compiler from reactive views to native Rust",
    after_help = "Exit codes: 0 clean, 1 denied, 2 internal error."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum HostProfile {
    /// Layout-only (taffy) profile.
    Taffy,
    /// Web-capable profile.
    Web,
}

impl From<HostProfile> for Profile {
    fn from(p: HostProfile) -> Self {
        match p {
            HostProfile::Taffy => Profile::Layout,
            HostProfile::Web => Profile::Web,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Admission + parity over the sources; no codegen (§12.1).
    Check {
        /// Source files or directories (.tsx/.vue).
        paths: Vec<PathBuf>,
        /// Promote Lint-tier diagnostics to Deny (CI gate).
        #[arg(long)]
        deny_warn: bool,
        /// Host style profile used for tiering.
        #[arg(long, value_enum, default_value = "taffy")]
        profile: HostProfile,
    },
    /// Emit Rust + source maps + IR + manifest (§9.5, fail-closed,
    /// digest-gated re-emission D-9).
    Build {
        /// Source files or directories (.tsx/.vue).
        paths: Vec<PathBuf>,
        /// Output directory.
        #[arg(short, long, default_value = "dist")]
        out: PathBuf,
        /// Host style profile used for tiering.
        #[arg(long, value_enum, default_value = "taffy")]
        profile: HostProfile,
    },
    /// Rebuild on source changes — digest-polled incremental passes (M3).
    Watch {
        /// Source files or directories (.tsx/.vue).
        paths: Vec<PathBuf>,
        /// Output directory.
        #[arg(short, long, default_value = "dist")]
        out: PathBuf,
        /// Host style profile used for tiering.
        #[arg(long, value_enum, default_value = "taffy")]
        profile: HostProfile,
        /// Run exactly one pass and exit (CI/tests).
        #[arg(long)]
        once: bool,
        /// Poll interval in milliseconds.
        #[arg(long, default_value_t = 300)]
        interval_ms: u64,
    },
    /// Long-form help for a diagnostic code (generated from the catalog, D-7).
    Explain {
        /// A stable code, e.g. LIG-E114.
        code: String,
    },
    /// Map a generated-Rust position back to source through a `.map.json`.
    Resolve {
        /// The emitted `.map.json` file.
        map: PathBuf,
        /// Generated line (1-based).
        line: u32,
        /// Generated column (1-based).
        col: u32,
    },
    /// Print the canonical View IR JSON for a source file (§5.6).
    Inspect {
        /// Source file (.tsx/.vue).
        path: PathBuf,
    },
    /// Environment checks: catalog, frontends, patch protocol, out-dir (M3).
    Doctor,
    /// Run the conformance/admission fixture suites and the fixture
    /// regenerators (the CI parity with the former xtask).
    Fixtures {
        #[command(subcommand)]
        which: FixtureCmd,
    },
    /// D-8 gate: committed codegen is byte-identical to a fresh generation.
    Reproducibility,
    /// S2/M3 gate: two-generation save→frame hot-patch budget (< 1 s per
    /// save) on the dummy host — chained generations + stub thunks.
    PatchSpike,
    /// D-9 gate: digest-gated re-emission + LIG-E020 cycle recovery (M3).
    Incremental,
    /// Regenerate the error-code registry + embedded catalog from
    /// docs/errors/*.md (D-7).
    ErrorsGen,
    /// §14.2 gate: measure the bench workloads in-process against the
    /// committed budget ledger (docs/benchmarks/budgets.toml). Time budgets
    /// are enforced in release runs; bytes rows everywhere.
    Budgets,
    /// §10 gate: the no_std proof — the structural contract plus bare-metal
    /// builds of lig-runtime and a generated unit.
    NoStd,
}

#[derive(Debug, Subcommand)]
enum FixtureCmd {
    /// Regenerate fixtures/generated/*.rs + the counter expected IR.
    Gen,
    /// Canonical-IR parity over fixtures/conformance/** (both frontend legs).
    Verify,
    /// Regenerate expected.ir.json for every conformance fixture (golden files).
    ParityGen,
    /// NFR-5 admission fixture harness over fixtures/admission/**.
    Admission,
    /// Rule table ↔ fixtures 1:1 (V.7).
    RulesSync,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Check {
            paths,
            deny_warn,
            profile,
        } => match run_check(&paths, deny_warn, profile.into()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(CliError::Denied) => ExitCode::from(1),
            Err(CliError::Internal) => ExitCode::from(2),
        },
        Command::Build {
            paths,
            out,
            profile,
        } => match run_build(&paths, &out, profile.into()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(CliError::Denied) => ExitCode::from(1),
            Err(CliError::Internal) => ExitCode::from(2),
        },
        Command::Watch {
            paths,
            out,
            profile,
            once,
            interval_ms,
        } => match run_watch(&paths, &out, profile.into(), once, interval_ms) {
            Ok(()) => ExitCode::SUCCESS,
            Err(CliError::Denied) => ExitCode::from(1),
            Err(CliError::Internal) => ExitCode::from(2),
        },
        Command::Explain { code } => {
            run_explain(&code);
            ExitCode::SUCCESS
        }
        Command::Resolve { map, line, col } => match run_resolve(&map, line, col) {
            Ok(()) => ExitCode::SUCCESS,
            Err(CliError::Denied) => ExitCode::from(1),
            Err(CliError::Internal) => ExitCode::from(2),
        },
        Command::Inspect { path } => match run_inspect(&path) {
            Ok(()) => ExitCode::SUCCESS,
            Err(CliError::Denied) => ExitCode::from(1),
            Err(CliError::Internal) => ExitCode::from(2),
        },
        Command::Doctor => {
            run_doctor();
            ExitCode::SUCCESS
        }
        Command::Fixtures { which } => match which {
            FixtureCmd::Gen => ExitCode::from(gates::fixtures_gen() as u8),
            FixtureCmd::Verify => ExitCode::from(gates::parity() as u8),
            FixtureCmd::ParityGen => ExitCode::from(gates::parity_gen() as u8),
            FixtureCmd::Admission => ExitCode::from(gates::admission() as u8),
            FixtureCmd::RulesSync => ExitCode::from(gates::rules_sync() as u8),
        },
        Command::Reproducibility => ExitCode::from(gates::reproducibility() as u8),
        Command::PatchSpike => ExitCode::from(gates::patch_spike() as u8),
        Command::Incremental => ExitCode::from(gates::incremental() as u8),
        Command::ErrorsGen => ExitCode::from(gates::errors_gen() as u8),
        Command::Budgets => ExitCode::from(budgets::budgets_gate() as u8),
        Command::NoStd => ExitCode::from(nostd::no_std_proof() as u8),
    }
}

enum CliError {
    Denied,
    Internal,
}

fn collect_sources(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            let mut found: Vec<PathBuf> = walk(p);
            found.sort();
            out.extend(found);
        } else {
            out.push(p.clone());
        }
    }
    out
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else if p.extension().is_some_and(|x| x == "tsx" || x == "vue") {
                out.push(p);
            }
        }
    }
    out
}

fn file_label(path: &Path) -> String {
    path.file_name().map_or_else(
        || "input.tsx".to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Read a source file, mapping I/O failure to the CLI's internal error.
fn read_source(path: &Path, what: &str) -> Result<String, CliError> {
    std::fs::read_to_string(path)
        .map_err(|e| eprintln!("lig {what}: {}: {e}", path.display()))
        .map_err(|()| CliError::Internal)
}

/// Build the session over the source set (the M3 entry point shared by
/// check/build/watch).
fn session_over(paths: &[PathBuf], what: &str) -> Result<Session, CliError> {
    let sources = collect_sources(paths);
    if sources.is_empty() {
        eprintln!("lig {what}: no .tsx/.vue sources given");
        return Err(CliError::Internal);
    }
    let mut session = Session::new();
    for path in &sources {
        let text = read_source(path, what)?;
        session.set_source(path, text);
    }
    Ok(session)
}

fn run_check(paths: &[PathBuf], deny_warn: bool, profile: Profile) -> Result<(), CliError> {
    let mut session = session_over(paths, "check")?;
    session.compile(profile);
    let mut denied = false;
    for path in session.sources() {
        let Some(unit) = session.unit(&path) else {
            continue;
        };
        let name = file_label(&path);
        if unit.admitted {
            let lint_count = unit
                .diagnostics
                .iter()
                .filter(|d| d.severity == lig_core::diagnostic::Severity::Warning)
                .count();
            if lint_count > 0 {
                println!(
                    "{name}: admitted with {lint_count} lint(s) ({} allowed constructs)",
                    unit.allowed
                );
                if deny_warn {
                    denied = true;
                    eprintln!("{name}: DENIED (--deny-warn promotes lints)");
                }
                for d in unit
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == lig_core::diagnostic::Severity::Warning)
                {
                    eprintln!("{}", render(&name, &unit.source, d));
                }
            } else {
                println!(
                    "{name}: admitted ({} constructs, no diagnostics)",
                    unit.allowed
                );
            }
        } else {
            denied = true;
            if unit.module.is_none() {
                eprintln!("{name}: DENIED at lowering");
            } else {
                eprintln!("{name}: DENIED");
            }
            for d in &unit.diagnostics {
                eprintln!("{}", render(&name, &unit.source, d));
            }
        }
    }
    if denied {
        Err(CliError::Denied)
    } else {
        Ok(())
    }
}

fn run_build(paths: &[PathBuf], out: &Path, profile: Profile) -> Result<(), CliError> {
    let mut session = session_over(paths, "build")?;
    session.compile(profile);

    // Fail-closed (BR-1): every unit compiles and admits BEFORE anything is
    // written. Codegen failures are internal errors; denials exit 1.
    let mut any_denied = false;
    for path in session.sources() {
        let Some(unit) = session.unit(&path) else {
            continue;
        };
        let name = file_label(&path);
        if let Some(e) = &unit.codegen_error {
            // CodegenFailure is a miette Diagnostic (M18): print the message
            // and its help-text (the fix, in words) like the emit seam does.
            eprintln!("lig build: {name}: codegen failed: {e}");
            eprintln!("  help: {}", e.help);
            return Err(CliError::Internal);
        }
        if !unit.admitted {
            any_denied = true;
            eprintln!("{name}: DENIED (zero output written — BR-1)");
            for d in &unit.diagnostics {
                eprintln!("{}", render(&name, &unit.source, d));
            }
        }
    }
    if any_denied {
        return Err(CliError::Denied);
    }

    std::fs::create_dir_all(out).map_err(|e| {
        eprintln!("lig build: {}: {e}", out.display());
        CliError::Internal
    })?;
    emit_and_report(&session, out, profile, false, "build")
}

/// The shared emission step of `build` and `watch`: digest-gated writes plus
/// the per-unit report lines.
fn emit_and_report(
    session: &Session,
    out: &Path,
    profile: Profile,
    prune: bool,
    what: &str,
) -> Result<(), CliError> {
    let units: Vec<&lig_incremental::CompiledUnit> = session
        .sources()
        .iter()
        .filter_map(|p| session.unit(p))
        .collect();
    // The ledger name rides the profile itself (M19): the mapping lives in
    // the library now (`Profile::name`), not in a per-consumer match — the
    // build-state/manifest spelling and the CLI flag share it.
    let emission =
        lig_incremental::emit::emit_units(out, &units, profile.name(), prune).map_err(|e| {
            // EmitError is a miette Diagnostic: print the message and its
            // help-text (the fix, in words) on the CLI surface too.
            eprintln!("lig {what}: {e}");
            eprintln!("  help: {}", e.help);
            CliError::Internal
        })?;
    for path in session.sources() {
        let Some(unit) = session.unit(&path) else {
            continue;
        };
        let name = file_label(&path);
        if emission.kept.iter().any(|k| k == &unit.stem) {
            println!("{name}: kept (digest unchanged — D-9)");
            continue;
        }
        let Some(module) = unit.module.as_ref() else {
            continue;
        };
        if let (Some(stats), true) = (
            emission.stats.get(&unit.stem),
            emission.written.iter().any(|w| w == &unit.stem),
        ) {
            println!(
                "{name}: built {} component(s); sourcemap verified ({} spans, {} reactions)",
                module.components.len(),
                stats.expr_spans_resolved,
                stats.reactions_linked
            );
        }
    }
    println!(
        "lig {what}: {} unit(s) → {} ({} kept, {} re-emitted)",
        units.len(),
        out.display(),
        emission.kept.len(),
        emission.written.len()
    );
    Ok(())
}

fn run_watch(
    paths: &[PathBuf],
    out: &Path,
    profile: Profile,
    once: bool,
    interval_ms: u64,
) -> Result<(), CliError> {
    let mut session = session_over(paths, "watch")?;
    session.compile(profile);
    std::fs::create_dir_all(out).map_err(|e| {
        eprintln!("lig watch: {}: {e}", out.display());
        CliError::Internal
    })?;
    emit_and_report(&session, out, profile, true, "watch")?;
    if once {
        return Ok(());
    }
    let interval = std::time::Duration::from_millis(interval_ms);
    let roots: Vec<PathBuf> = paths.to_vec();
    let mut pass = 0u64;
    println!(
        "lig watch: watching {} source(s) — Ctrl-C to stop",
        session.sources().len()
    );
    loop {
        std::thread::sleep(interval);
        // The tick, as one sync call (the watch-sync seam): read the fresh
        // snapshot and hand it to the session — additions, edits, and
        // removals come back as one report. Explicit roots are kept even
        // when missing from disk, and a present-but-unreadable scanned
        // source keeps its last registered text (re-fed from the session).
        let seen = collect_sources(&roots);
        let mut fresh: Vec<(PathBuf, String)> = Vec::with_capacity(seen.len());
        for path in &seen {
            match std::fs::read_to_string(path) {
                Ok(text) => fresh.push((path.clone(), text)),
                Err(_) => {
                    if let Some(text) = session.source_text(path) {
                        fresh.push((path.clone(), text.to_string()));
                    }
                }
            }
        }
        let sync_report = session.sync(fresh, &roots);
        if sync_report.is_empty() {
            continue;
        }
        pass += 1;
        let report = session.compile(profile);
        // BR-1 for watch: a denied pass never touches dist — the last good
        // output stays live.
        let denied: Vec<_> = session
            .units()
            .iter()
            .filter(|u| !u.admitted)
            .map(|u| u.path.clone())
            .collect();
        if !denied.is_empty() {
            for path in &denied {
                let Some(unit) = session.unit(path) else {
                    continue;
                };
                let name = file_label(path);
                eprintln!("{name}: DENIED (dist keeps the last good output)");
                for d in &unit.diagnostics {
                    eprintln!("{}", render(&name, &unit.source, d));
                }
            }
            if once {
                return Err(CliError::Denied);
            }
            continue;
        }
        emit_and_report(&session, out, profile, true, "watch")?;
        println!(
            "lig watch pass {pass} (rev {}): {} recompiled, {} reused in {:.1} ms",
            report.revision,
            report.recompiled.len(),
            report.reused.len(),
            report.elapsed.as_secs_f64() * 1000.0
        );
    }
}

fn run_explain(code: &str) {
    // The catalog Markdown ships in two places: next to the repo (reviewed
    // like code) and embedded in the binary (M3 — `lig doctor` hardening,
    // works from any working directory).
    let candidates = [
        PathBuf::from(format!("docs/errors/{code}.md")),
        PathBuf::from(format!("../docs/errors/{code}.md")),
        PathBuf::from(format!("../../docs/errors/{code}.md")),
    ];
    for c in &candidates {
        if let Ok(md) = std::fs::read_to_string(c) {
            println!("{md}");
            match lig_core::error_code::lookup(code) {
                Some(e) => println!("details: {}", lig_core::error_code::url(e.code)),
                None => println!("details: (code not in the compiled registry)"),
            }
            return;
        }
    }
    if let Some(doc) = lig_core::error_code::catalog_doc(code) {
        println!("{doc}");
        match lig_core::error_code::lookup(code) {
            Some(e) => println!("details: {}", lig_core::error_code::url(e.code)),
            None => println!("details: (code not in the compiled registry)"),
        }
        return;
    }
    match lig_core::error_code::lookup(code) {
        Some(e) => {
            println!("{code} — {}", e.title);
            println!(
                "stage: {}{}",
                e.stage,
                if e.reserved { " (reserved)" } else { "" }
            );
            println!("details: {}", lig_core::error_code::url(e.code));
            println!("(catalog markdown not found next to the binary; showing registry summary)");
        }
        None => {
            println!("{code}: unknown code (see https://ligature.dev/e/ for the catalog)");
        }
    }
}

fn run_resolve(map_path: &PathBuf, line: u32, col: u32) -> Result<(), CliError> {
    let json = std::fs::read_to_string(map_path).map_err(|e| {
        eprintln!("lig resolve: {}: {e}", map_path.display());
        CliError::Internal
    })?;
    let map = oxc_sourcemap::SourceMap::from_json_string(&json).map_err(|e| {
        eprintln!("lig resolve: {}: {e}", map_path.display());
        CliError::Internal
    })?;
    let table = map.generate_lookup_table();
    // CLI positions are 1-based; VLQ maps are 0-based.
    let token = map
        .lookup_token(&table, line.saturating_sub(1), col.saturating_sub(1))
        .ok_or_else(|| {
            eprintln!("lig resolve: no mapping at {line}:{col}");
            CliError::Denied
        })?;
    let file = token
        .get_source_id()
        .and_then(|id| map.get_source(id))
        .unwrap_or("?");
    let (sline, scol) = (token.get_src_line() + 1, token.get_src_col() + 1);
    let name = token
        .get_name_id()
        .and_then(|nid| map.get_name(nid))
        .unwrap_or("-");
    println!("{file}:{sline}:{scol} (ir node: {name})");
    Ok(())
}

fn run_inspect(path: &PathBuf) -> Result<(), CliError> {
    let source = std::fs::read_to_string(path).map_err(|e| {
        eprintln!("lig inspect: {}: {e}", path.display());
        CliError::Internal
    })?;
    let name = path.file_name().map_or("input.tsx".into(), |n| {
        std::borrow::Cow::from(n.to_string_lossy().into_owned())
    });
    match lig_incremental::lower_source(&source, &name) {
        Ok(m) => {
            println!("{}", m.canonical_json_pretty());
            Ok(())
        }
        Err(diags) => {
            for d in &diags {
                eprintln!("{}", render(&name, &source, d));
            }
            Err(CliError::Denied)
        }
    }
}

fn run_doctor() {
    println!("lig doctor — environment checks (§12.1, M3)");
    let mut failures = 0usize;

    // Version + IR contract.
    println!(
        "  ✓ lig {} · view IR v{}",
        env!("CARGO_PKG_VERSION"),
        lig_ir::VIEW_IR_VERSION
    );

    // Frontends.
    println!("  ✓ frontends: solid (TSX), vue (SFC)");

    // Embedded catalog (M3 hardening: `lig explain` works anywhere).
    let registry = lig_core::error_code::registry();
    let embedded = registry
        .iter()
        .filter(|e| lig_core::error_code::catalog_doc(e.code).is_some())
        .count();
    if embedded == registry.len() && !registry.is_empty() {
        println!(
            "  ✓ error catalog embedded ({} codes, {} reserved)",
            registry.len(),
            registry.iter().filter(|e| e.reserved).count()
        );
    } else {
        failures += 1;
        println!(
            "  ✗ error catalog incomplete ({embedded}/{} embedded) — run `lig errors-gen`",
            registry.len()
        );
    }

    // Patch protocol support (unix dlopen protocol, §11.3).
    if cfg!(windows) {
        println!("  • patch protocol: unavailable on windows (dlopen protocol is unix-only)");
    } else {
        println!("  ✓ patch protocol: dlopen jump-table surface available");
    }

    // M4: the budget ledger (§14.2) and the no_std proof environment (§10).
    match budgets::load_ledger_for_doctor() {
        Ok(rows) => println!("  ✓ budget ledger: {rows} row(s) (docs/benchmarks/budgets.toml)"),
        Err(e) => {
            failures += 1;
            println!("  ✗ budget ledger: {e}");
        }
    }
    let (installed, total) = nostd::installed_targets();
    if installed == total {
        println!(
            "  ✓ no_std proof targets installed ({})",
            nostd::target_names().join(", ")
        );
    } else {
        println!(
            "  • no_std proof targets: {installed}/{total} installed — `rustup target add {}` \
             enables `lig no-std`",
            nostd::target_names().join(" ")
        );
    }

    // Out-dir writability.
    let probe = std::env::temp_dir().join(format!("lig-doctor-{}", std::process::id()));
    match std::fs::create_dir_all(&probe).and_then(|()| std::fs::remove_dir(&probe)) {
        Ok(()) => println!(
            "  ✓ writable output root ({})",
            std::env::temp_dir().display()
        ),
        Err(e) => {
            failures += 1;
            println!("  ✗ cannot write output root: {e}");
        }
    }

    if failures == 0 {
        println!("lig doctor: all checks pass");
    } else {
        println!("lig doctor: {failures} check(s) failed");
    }
}
