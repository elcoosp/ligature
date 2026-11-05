//! The `lig` CLI (§12.1): `check`, `build`, `explain`, `resolve`, `inspect`,
//! plus the CI gates (`fixtures …`, `reproducibility`, `patch-spike`,
//! `errors-gen`) that M2 folded in from the former xtask. M2 surface — dual
//! frontends (Solid TSX / Vue SFC), admission tiers, codegen, source maps.
//! Exit codes: 0 = clean, 1 = denied, 2 = internal error.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod gates;

use clap::{Parser, Subcommand, ValueEnum};
use lig_admission::Profile;
use lig_core::diagnostic::render;
use lig_ir::ViewModule;
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
    /// Emit Rust + source maps + IR + manifest (§9.5, fail-closed).
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
    /// Run the conformance/admission fixture suites and the fixture
    /// regenerators (the CI parity with the former xtask).
    Fixtures {
        #[command(subcommand)]
        which: FixtureCmd,
    },
    /// D-8 gate: committed codegen is byte-identical to a fresh generation.
    Reproducibility,
    /// S2 gate: save→frame hot-patch budget (< 1 s) on the dummy host.
    PatchSpike,
    /// Regenerate the error-code registry from docs/errors/*.md (D-7).
    ErrorsGen,
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
        Command::Fixtures { which } => match which {
            FixtureCmd::Gen => ExitCode::from(gates::fixtures_gen() as u8),
            FixtureCmd::Verify => ExitCode::from(gates::parity() as u8),
            FixtureCmd::ParityGen => ExitCode::from(gates::parity_gen() as u8),
            FixtureCmd::Admission => ExitCode::from(gates::admission() as u8),
            FixtureCmd::RulesSync => ExitCode::from(gates::rules_sync() as u8),
        },
        Command::Reproducibility => ExitCode::from(gates::reproducibility() as u8),
        Command::PatchSpike => ExitCode::from(gates::patch_spike() as u8),
        Command::ErrorsGen => ExitCode::from(gates::errors_gen() as u8),
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

/// Lower a source file with the frontend its extension selects (§6: `.tsx`
/// → Solid, `.vue` → Vue SFC). The canonical IR is the same contract.
fn lower_source(
    source: &str,
    name: &str,
) -> Result<lig_ir::ViewModule, Vec<lig_core::diagnostic::LigDiagnostic>> {
    if Path::new(name)
        .extension()
        .is_some_and(|x| x.eq_ignore_ascii_case("vue"))
    {
        lig_parse_vue::lower_module(source, name)
    } else {
        lig_parse_solid::lower_module(source, name)
    }
}

fn run_check(paths: &[PathBuf], deny_warn: bool, profile: Profile) -> Result<(), CliError> {
    let sources = collect_sources(paths);
    if sources.is_empty() {
        eprintln!("lig check: no .tsx/.vue sources given");
        return Err(CliError::Internal);
    }
    let mut denied = false;
    for path in &sources {
        let source = std::fs::read_to_string(path)
            .map_err(|e| eprintln!("lig check: {}: {e}", path.display()))
            .map_err(|()| CliError::Internal)?;
        let name = path.file_name().map_or("input.tsx".into(), |n| {
            std::borrow::Cow::from(n.to_string_lossy().into_owned())
        });
        match lower_source(&source, &name) {
            Ok(m) => {
                let report = lig_admission::admit(&m, profile);
                let report = if deny_warn {
                    report.with_lints_denied()
                } else {
                    report
                };
                if report.is_admitted() {
                    let lint_count = report.lints().count();
                    if lint_count > 0 {
                        println!(
                            "{}: admitted with {lint_count} lint(s) ({} allowed constructs)",
                            name, report.allowed
                        );
                        for d in report.lints() {
                            eprintln!("{}", render(&name, &source, d));
                        }
                    } else {
                        println!(
                            "{}: admitted ({} constructs, no diagnostics)",
                            name, report.allowed
                        );
                    }
                } else {
                    denied = true;
                    eprintln!("{name}: DENIED");
                    for d in report.diagnostics {
                        eprintln!("{}", render(&name, &source, &d));
                    }
                }
            }
            Err(diags) => {
                denied = true;
                eprintln!("{name}: DENIED at lowering");
                for d in &diags {
                    eprintln!("{}", render(&name, &source, d));
                }
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
    let sources = collect_sources(paths);
    if sources.is_empty() {
        eprintln!("lig build: no .tsx/.vue sources given");
        return Err(CliError::Internal);
    }
    // Fail-closed (BR-1): check everything BEFORE writing anything.
    let mut built: Vec<(String, BuiltUnit)> = Vec::new();
    for path in &sources {
        let source = std::fs::read_to_string(path)
            .map_err(|e| eprintln!("lig build: {}: {e}", path.display()))
            .map_err(|()| CliError::Internal)?;
        // Unique stems: `input.solid.tsx` fixtures take their directory name.
        let stem = if path
            .file_stem()
            .is_some_and(|s| s.to_string_lossy().contains('.'))
        {
            path.parent().and_then(|p| p.file_name()).map_or_else(
                || "module".to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        } else {
            path.file_stem()
                .map_or("module".into(), |n| n.to_string_lossy().into_owned())
        };
        let name = path.file_name().map_or("input.tsx".into(), |n| {
            std::borrow::Cow::from(n.to_string_lossy().into_owned())
        });
        let module = match lower_source(&source, &name) {
            Ok(m) => m,
            Err(diags) => {
                eprintln!("{name}: DENIED (zero output written — BR-1)");
                for d in &diags {
                    eprintln!("{}", render(&name, &source, d));
                }
                return Err(CliError::Denied);
            }
        };
        let report = lig_admission::admit(&module, profile);
        if !report.is_admitted() {
            eprintln!("{name}: DENIED (zero output written — BR-1)");
            for d in &report.diagnostics {
                eprintln!("{}", render(&name, &source, d));
            }
            return Err(CliError::Denied);
        }
        // One generated file per source file, all components inside (§9.5).
        let generated = lig_codegen::generate_module(&module).map_err(|e| {
            eprintln!("lig build: {name}: codegen failed: {e}");
            CliError::Internal
        })?;
        let links = generated.links;
        // NFR-6: source maps verified before emission.
        let text = generated.text;
        let stats = lig_sourcemap::verify(&module, &source, &text, &links).map_err(|e| {
            eprintln!("lig build: {name}: sourcemap verify failed: {e}");
            CliError::Internal
        })?;
        println!(
            "{name}: built {} component(s); sourcemap verified ({} spans, {} reactions)",
            module.components.len(),
            stats.expr_spans_resolved,
            stats.reactions_linked
        );
        built.push((
            stem.clone(),
            BuiltUnit {
                module,
                source,
                text,
                links,
            },
        ));
    }
    // Emit (atomic-ish: temp dir then rename per file).
    std::fs::create_dir_all(out)
        .and_then(|()| std::fs::create_dir_all(out.join("ir")))
        .and_then(|()| std::fs::create_dir_all(out.join("maps")))
        .map_err(|e| {
            eprintln!("lig build: {}: {e}", out.display());
            CliError::Internal
        })?;
    let mut manifest_rows: Vec<String> = vec![
        "# generated by lig build — do not edit".to_string(),
        format!(
            "profile = \"{}\"",
            if profile == Profile::Web {
                "web"
            } else {
                "taffy"
            }
        ),
        format!("units = {}", built.len()),
    ];
    for (stem, unit) in &built {
        let rs = out.join(format!("{stem}.rs"));
        write_atomic(&rs, unit.text.as_bytes())?;
        let ir_json = format!("{}\n", unit.module.canonical_json_pretty());
        write_atomic(
            &out.join("ir").join(format!("{stem}.json")),
            ir_json.as_bytes(),
        )?;
        let src_map = lig_sourcemap::src_to_ir(&unit.module, &unit.source);
        let ir_map =
            lig_sourcemap::ir_to_rust(&unit.module, &unit.text, &unit.links).map_err(|e| {
                eprintln!("lig build: {stem}: {e}");
                CliError::Internal
            })?;
        let merged = lig_sourcemap::chain(&src_map, &ir_map);
        write_atomic(
            &out.join("maps").join(format!("{stem}.map.json")),
            merged.to_json_string().as_bytes(),
        )?;
        manifest_rows.push(format!("{stem} = \"{stem}.rs\""));
    }
    write_atomic(
        &out.join("manifest.toml"),
        manifest_rows.join("\n").as_bytes(),
    )?;
    println!("lig build: {} unit(s) → {}", built.len(), out.display());
    Ok(())
}

struct BuiltUnit {
    module: ViewModule,
    source: String,
    text: String,
    links: lig_codegen::GenLinks,
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, bytes)
        .and_then(|()| std::fs::rename(&tmp, path))
        .is_err()
    {
        eprintln!("lig build: failed to write {}", path.display());
        return Err(CliError::Internal);
    }
    Ok(())
}

fn run_explain(code: &str) {
    // The catalog Markdown ships with the repo; look relative to the binary
    // and the working directory (lig doctor hardens this in M3).
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
    let map = oxc_sourcemap::SourceMap::from_json_string(&json)
        .map_err(|e| {
            eprintln!("lig resolve: {}: {e}", map_path.display());
            CliError::Internal
        })
        .map_err(|_| CliError::Internal)?;
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
    match lower_source(&source, &name) {
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
