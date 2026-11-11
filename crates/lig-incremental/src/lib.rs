//! Salsa-style incremental compilation sessions (§11.2, D-9): the M3 engine
//! behind `lig build`, `lig watch`, and the digest gate.
//!
//! Shape: **inputs** are source texts (`Session::set_source`), **queries** are
//! the pipeline stages (`lowered`, `generated`), and every query result is a
//! **memo validated by recorded dependency digests** — recompute only when a
//! digest changed, reuse byte-identical work otherwise. Revisions count input
//! edits (salsa's revision counter, dependency-free by ADR-015: the same
//! memo/revision/cycle-recovery semantics over `u64` FNV digests instead of
//! the salsa runtime).
//!
//! Cycle recovery (§11.1, LIG-E020): a cyclic component-use graph has no
//! fragment build order; the `build-order` stage surfaces it as a Deny
//! diagnostic in the same pass that would otherwise recurse forever — never a
//! hang.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod emit;
pub mod observe;

use lig_admission::{Profile, admit};
use lig_codegen::{GenLinks, generate_module};
use lig_core::diagnostic::LigDiagnostic;
use lig_core::hash::fnv1a64;
use lig_core::span::Span;
use lig_ir::ViewModule;
use lig_reactivity::component_build_order;
use observe::{SessionObserver, Stage, StageEvent, StageOutcome};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The FNV-1a 64 digest that gates every memo (D-9's currency).
#[must_use]
pub fn digest(bytes: &[u8]) -> u64 {
    fnv1a64(bytes)
}

/// Lower a source file with the frontend its extension selects (§6: `.tsx`
/// → Solid, `.vue` → Vue SFC). The canonical IR is the same contract. Any
/// other extension is a precise `LIG-E001` — before M18 unknown extensions
/// silently lowered as Solid and failed far from the cause.
pub fn lower_source(
    source: &str,
    name: &str,
) -> Result<ViewModule, Vec<lig_core::diagnostic::LigDiagnostic>> {
    let ext = Path::new(name)
        .extension()
        .map(|x| x.to_string_lossy().into_owned())
        .map(|x| x.to_ascii_lowercase());
    match ext.as_deref() {
        Some("vue") => lig_parse_vue::lower_module(source, name),
        Some("tsx") => lig_parse_solid::lower_module(source, name),
        _ => Err(vec![LigDiagnostic::error(
            "LIG-E001",
            format!(
                "unsupported source extension for `{name}` — pass a `.tsx` file (Solid TSX) \
                 or a `.vue` file (Vue SFC)"
            ),
            Span::new(0, 0),
        )]),
    }
}

/// The generated-unit stem for a source path: dotted stems (`input.solid.tsx`
/// fixture legs) take the parent directory's name (§9.5 emission naming).
#[must_use]
pub fn unit_stem(path: &Path) -> String {
    if path
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
    }
}

/// One compiled unit: everything the emitter needs, produced through the
/// memoized queries.
#[derive(Debug, Clone)]
pub struct CompiledUnit {
    /// Source path (as registered in the session).
    pub path: PathBuf,
    /// The emitted-unit stem (`unit_stem`).
    pub stem: String,
    /// The source text (embedded in the src→IR map content).
    pub source: String,
    /// The lower query's result (`None` = lowering was denied).
    pub module: Option<ViewModule>,
    /// The generate query's result (`Some` iff admitted — fail-closed BR-1).
    pub generated: Option<GeneratedOutput>,
    /// Admission/lowering/cycle diagnostics for this unit, post-profile.
    pub diagnostics: Vec<LigDiagnostic>,
    /// Whether the unit passed admission (and cycles) — emit gate.
    pub admitted: bool,
    /// Allow-tier constructs the admission report counted.
    pub allowed: usize,
    /// Codegen failure (an internal error — exit 2 in the CLI), if any.
    pub codegen_error: Option<CodegenFailure>,
}

impl CompiledUnit {
    /// FNV-1a 64 of the unit's source text (the D-9 input digest).
    #[must_use]
    pub fn source_hash(&self) -> u64 {
        digest(self.source.as_bytes())
    }
}

/// Codegen's output plus the digest that gates its re-emission (D-9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedOutput {
    /// The generated Rust text (prettyplease; byte-reproducible, D-8).
    pub text: String,
    /// IR → generated-symbol links (§9.4 source maps).
    pub links: GenLinks,
    /// FNV-1a 64 of `text` — two units re-emit iff this differs.
    pub digest: u64,
}

/// A codegen failure on one unit (the session's typed codegen seam, M18):
/// which unit failed, why, and the fix in words — a [`miette::Diagnostic`]
/// with a stable code (`ligature::codegen`) and help-text, so embedded
/// consumers get the same quality of failure report the emit seam (M16)
/// and the CLI print. Before M18 this seam carried a bare `String`.
#[derive(Debug, Clone)]
pub struct CodegenFailure {
    /// The unit path the failure is about.
    pub subject: String,
    /// What went wrong (renders as the error line).
    pub message: String,
    /// The fix, in words (renders as `help:`).
    pub help: String,
    /// The underlying codegen error, when one caused this.
    source: Option<lig_codegen::CodegenError>,
}

impl CodegenFailure {
    /// Map a codegen error onto the typed seam: the message carries the
    /// precise backstop reason; the help names the fix for the documented
    /// backstop families, or the report path for internal bugs.
    fn from(unit: &Path, e: &lig_codegen::CodegenError) -> Self {
        let message = e.to_string();
        let help = match e {
            lig_codegen::CodegenError::Unsupported(what) => {
                if what.contains("per-item slot content") {
                    "binds inside per-item slot content read the item/index only — read parent \
                     state through a keyed component (M13's per-item instance identity), or \
                     hoist the bind out of the list body"
                        .to_string()
                } else if what.contains("fill would never mount") {
                    "fills for list-body outlets never mount (items do not exist at the \
                     parent's build) — author the content as the outlet's own fallback, which \
                     the per-item replay mounts (M14)"
                        .to_string()
                } else {
                    "the codegen backstop denies a construct the frontends admit — the drawn \
                     boundary is docs/admission/rules.md and the catalog is `lig explain`; \
                     report the shape with the session revision if the docs admit it"
                        .to_string()
                }
            }
            lig_codegen::CodegenError::Schedule(_) => {
                "the const schedule is invalid (cycle or capacity) — an internal bug: report it \
                 with the session revision from `lig doctor`"
                    .to_string()
            }
            lig_codegen::CodegenError::Readability(_) => {
                "the generated unit exceeded the readability budget — a codegen bug, not a lint \
                 (§9.3): report it with the session revision"
                    .to_string()
            }
        };
        Self {
            subject: unit.display().to_string(),
            message,
            help,
            source: Some(e.clone()),
        }
    }
}

impl core::fmt::Display for CodegenFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CodegenFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e as &(dyn std::error::Error + 'static))
    }
}

impl miette::Diagnostic for CodegenFailure {
    fn code<'a>(&'a self) -> Option<Box<dyn core::fmt::Display + 'a>> {
        Some(Box::new("ligature::codegen"))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn core::fmt::Display + 'a>> {
        Some(Box::new(&self.help))
    }
}

impl GeneratedOutput {
    /// The placeholder for units with no codegen output (denied/lowering
    /// failure): empty text, zero digest — never emitted (BR-1).
    #[must_use]
    pub fn denied() -> Self {
        Self {
            text: String::new(),
            links: GenLinks::default(),
            digest: 0,
        }
    }
}

/// The result of one compile pass over the whole session.
#[derive(Debug, Default, Clone)]
pub struct CompileReport {
    /// Session revision after this pass (bumped by `set_source` edits).
    pub revision: u64,
    /// Units whose queries recomputed this pass (digest or profile changed).
    pub recompiled: Vec<PathBuf>,
    /// Units served entirely from valid memos (digest-gated reuse).
    pub reused: Vec<PathBuf>,
    /// Wall clock for the pass.
    pub elapsed: std::time::Duration,
}

impl CompileReport {
    /// Whether every query was served from memos (the "kept everything" line
    /// `lig watch` prints).
    #[must_use]
    pub fn all_reused(&self) -> bool {
        self.recompiled.is_empty()
    }
}

/// One path's memo table entry — the dependency digests each query recorded
/// plus the value it produced.
#[derive(Debug, Clone)]
struct UnitMemo {
    lowered: Option<LoweredMemo>,
    generated: Option<GeneratedMemo>,
    unit: Option<CompiledUnit>,
}

impl UnitMemo {
    fn empty() -> Self {
        Self {
            lowered: None,
            generated: None,
            unit: None,
        }
    }
}

#[derive(Debug, Clone)]
struct LoweredMemo {
    source_digest: u64,
    outcome: Result<ViewModule, Vec<LigDiagnostic>>,
}

#[derive(Debug, Clone)]
struct GeneratedMemo {
    source_digest: u64,
    profile: Profile,
}

/// The diff one [`Session::sync`] pass applied to the source set: the paths
/// that entered (added), changed text (edited), or left (removed). Paths are
/// sorted; empty when the snapshot matched the session exactly.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Sources the snapshot introduced.
    pub added: Vec<PathBuf>,
    /// Sources whose registered text changed (their memos invalidate
    /// through the digest gate on the next compile).
    pub edited: Vec<PathBuf>,
    /// Sources absent from the snapshot and not in `keep` — dropped
    /// together with their memos.
    pub removed: Vec<PathBuf>,
}

impl SyncReport {
    /// Whether the snapshot matched the session exactly (a watch tick with
    /// nothing to do).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.edited.is_empty() && self.removed.is_empty()
    }
}

/// An incremental compilation session: inputs + memos + revision counter.
#[derive(Debug, Default)]
pub struct Session {
    sources: BTreeMap<PathBuf, SourceInput>,
    memos: BTreeMap<PathBuf, UnitMemo>,
    revision: u64,
}

#[derive(Debug, Clone)]
struct SourceInput {
    text: String,
    digest: u64,
}

impl Session {
    /// An empty session.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) a source. Bumps the revision iff the text
    /// actually changed — no-op edits never invalidate memos (D-9).
    pub fn set_source(&mut self, path: impl Into<PathBuf>, text: impl Into<String>) {
        let path = path.into();
        let text = text.into();
        let digest = digest(text.as_bytes());
        let changed = self
            .sources
            .get(&path)
            .is_none_or(|old| old.digest != digest);
        self.sources.insert(path, SourceInput { text, digest });
        if changed {
            self.revision += 1;
        }
    }

    /// Drop a source and its memos.
    pub fn remove_source(&mut self, path: &Path) {
        self.sources.remove(path);
        self.memos.remove(path);
        self.revision += 1;
    }

    /// Registered source paths, sorted.
    #[must_use]
    pub fn sources(&self) -> Vec<PathBuf> {
        self.sources.keys().cloned().collect()
    }

    /// The current revision counter.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The registered text for `path`, if any — the input accessor watch
    /// integrations use to re-feed the last known text of a file that still
    /// exists but could not be read this tick (it stays registered with its
    /// old text rather than leaving the session).
    #[must_use]
    pub fn source_text(&self, path: &Path) -> Option<&str> {
        self.sources.get(path).map(|s| s.text.as_str())
    }

    /// Diff a fresh source snapshot (path → text) into the session — the
    /// watch tick as one seam call. Additions and changed texts register
    /// with the D-9 no-op discipline (identical texts are not edits and
    /// never invalidate memos); session sources absent from the snapshot
    /// are dropped with their memos, unless listed in `keep` — the CLI
    /// watch passes its explicit root paths so a deleted root keeps its
    /// last registered text instead of being pruned.
    ///
    /// Returns the [`SyncReport`] of what changed (paths sorted; a
    /// duplicated snapshot path takes its last text).
    pub fn sync<I, S>(&mut self, fresh: I, keep: &[PathBuf]) -> SyncReport
    where
        I: IntoIterator<Item = (PathBuf, S)>,
        S: Into<String>,
    {
        let mut report = SyncReport::default();
        let fresh: BTreeMap<PathBuf, String> =
            fresh.into_iter().map(|(p, t)| (p, t.into())).collect();
        for (path, text) in &fresh {
            let text_digest = digest(text.as_bytes());
            let unchanged = self
                .sources
                .get(path)
                .is_some_and(|old| old.digest == text_digest);
            if unchanged {
                continue;
            }
            let added = !self.sources.contains_key(path);
            self.set_source(path.clone(), text.clone());
            if added {
                report.added.push(path.clone());
            } else {
                report.edited.push(path.clone());
            }
        }
        for path in self.sources.keys().cloned().collect::<Vec<_>>() {
            if !fresh.contains_key(&path) && !keep.contains(&path) {
                self.remove_source(&path);
                report.removed.push(path);
            }
        }
        report
    }

    /// The compiled unit for `path` after the last [`Session::compile`] pass.
    #[must_use]
    pub fn unit(&self, path: &Path) -> Option<&CompiledUnit> {
        self.memos.get(path).and_then(|m| m.unit.as_ref())
    }

    /// All compiled units after the last pass, in path order.
    #[must_use]
    pub fn units(&self) -> Vec<&CompiledUnit> {
        self.memos
            .values()
            .filter_map(|m| m.unit.as_ref())
            .collect()
    }

    /// The `lowered` query: parse + lower through the frontend the extension
    /// selects, memoized on the source digest. Fires the `lower` stage event
    /// — `Computed`/`Failed` with the stage's wall clock on a recompute,
    /// `Reused` at zero when the memo still validated (D-9).
    fn query_lowered(
        &mut self,
        path: &Path,
        source: &str,
        source_digest: u64,
        observer: &mut Option<&mut dyn SessionObserver>,
    ) -> &LoweredMemo {
        let valid = self
            .memos
            .get(path)
            .and_then(|m| m.lowered.as_ref())
            .is_some_and(|m| m.source_digest == source_digest);
        if valid {
            fire_stage(
                observer,
                Stage::Lower,
                path,
                StageOutcome::Reused,
                Duration::ZERO,
            );
        } else {
            let t0 = Instant::now();
            let lower_guard = tracing::info_span!("lower", unit = %path.display()).entered();
            let outcome = lower_source(source, &file_name(path));
            let lowered = outcome.is_ok();
            let memo = UnitMemo {
                lowered: Some(LoweredMemo {
                    source_digest,
                    outcome,
                }),
                generated: None,
                unit: None,
            };
            self.memos.insert(path.to_path_buf(), memo);
            drop(lower_guard);
            fire_stage(
                observer,
                Stage::Lower,
                path,
                if lowered {
                    StageOutcome::Computed
                } else {
                    StageOutcome::Failed
                },
                t0.elapsed(),
            );
        }
        self.memos
            .get_mut(path)
            .and_then(|m| m.lowered.as_ref())
            .expect("memo just validated or inserted")
    }

    /// The `generated` query: admission (profile-dependent) → build-order
    /// cycle check (LIG-E020) → codegen, memoized on source digest + profile.
    /// Fires the `admit` and `codegen` stage events — `Reused` for both when
    /// the memo still validated, the stages' own events on a recompute.
    fn query_generated(
        &mut self,
        path: &Path,
        source_digest: u64,
        profile: Profile,
        observer: &mut Option<&mut dyn SessionObserver>,
    ) -> &GeneratedMemo {
        let valid = self
            .memos
            .get(path)
            .and_then(|m| m.generated.as_ref())
            .is_some_and(|m| m.source_digest == source_digest && m.profile == profile);
        if valid {
            for stage in [Stage::Admit, Stage::Codegen] {
                fire_stage(observer, stage, path, StageOutcome::Reused, Duration::ZERO);
            }
        } else {
            // The computed output rides the unit memo (`CompiledUnit::
            // generated` / `codegen_error`); the generated memo only ever
            // records the memo key.
            let _ = self.compute_generated(path, profile, observer);
            let entry = self
                .memos
                .entry(path.to_path_buf())
                .or_insert_with(UnitMemo::empty);
            entry.generated = Some(GeneratedMemo {
                source_digest,
                profile,
            });
        }
        self.memos
            .get_mut(path)
            .and_then(|m| m.generated.as_ref())
            .expect("memo just validated or inserted")
    }

    /// Compute (not memoize) the `generated` query: stage-by-stage, storing
    /// the resulting [`CompiledUnit`] on the memo entry. The `admit` and
    /// `codegen` stages report their events (with their own wall clocks) to
    /// the observer as they finish — a stage that never runs reports
    /// nothing.
    fn compute_generated(
        &mut self,
        path: &Path,
        profile: Profile,
        observer: &mut Option<&mut dyn SessionObserver>,
    ) -> Result<GeneratedOutput, CodegenFailure> {
        // Stage 1: the lowered module (owned clone — borrow-split from the
        // memo table below; lowering is the memoized expensive stage).
        let lowered = {
            let memo = self
                .memos
                .get(path)
                .and_then(|m| m.lowered.as_ref())
                .expect("query_lowered ran before query_generated");
            memo.outcome.clone()
        };
        let module = match lowered {
            Ok(m) => m,
            Err(diags) => {
                let unit = CompiledUnit {
                    path: path.to_path_buf(),
                    stem: unit_stem(path),
                    source: self.sources[path].text.clone(),
                    module: None,
                    generated: None,
                    diagnostics: diags,
                    admitted: false,
                    allowed: 0,
                    codegen_error: None,
                };
                self.memos
                    .get_mut(path)
                    .expect("lowered memo entry exists")
                    .unit = Some(unit);
                return Ok(GeneratedOutput::denied());
            }
        };

        // Stage 2: admission (profile tiering) + build-order cycle recovery
        // (LIG-E020) — one observable `admit` span over the tiering work.
        let admit_guard = tracing::info_span!("admit", unit = %path.display()).entered();
        let admit_t0 = Instant::now();
        let report = admit(&module, profile);
        let mut diagnostics: Vec<LigDiagnostic> = Vec::new();
        let mut admitted = report.is_admitted();
        let allowed = report.allowed;
        diagnostics.extend(report.diagnostics);

        // Stage 3: build-order cycle recovery (LIG-E020) — the Deny replaces
        // the recursion a cyclic build order would induce downstream.
        if admitted && let Err(cycle) = component_build_order(&module) {
            admitted = false;
            diagnostics.push(cycle_diagnostic(&module, &cycle));
        }

        let source_text = self.sources[path].text.clone();
        let admit_elapsed = admit_t0.elapsed();
        drop(admit_guard);
        if !admitted {
            let unit = CompiledUnit {
                path: path.to_path_buf(),
                stem: unit_stem(path),
                source: source_text,
                module: Some(module),
                generated: None,
                diagnostics,
                admitted: false,
                allowed,
                codegen_error: None,
            };
            self.memos
                .get_mut(path)
                .expect("lowered memo entry exists")
                .unit = Some(unit);
            fire_stage(
                observer,
                Stage::Admit,
                path,
                StageOutcome::Failed,
                admit_elapsed,
            );
            return Ok(GeneratedOutput::denied());
        }
        fire_stage(
            observer,
            Stage::Admit,
            path,
            StageOutcome::Computed,
            admit_elapsed,
        );

        // Stage 4: codegen (D-8 byte-reproducible).
        let codegen_t0 = Instant::now();
        let codegen_guard = tracing::info_span!("codegen", unit = %path.display()).entered();
        let generated = generate_module(&module);
        let codegen_elapsed = codegen_t0.elapsed();
        drop(codegen_guard);
        match generated {
            Ok(gm) => {
                let text_digest = digest(gm.text.as_bytes());
                let out = GeneratedOutput {
                    text: gm.text.clone(),
                    links: gm.links.clone(),
                    digest: text_digest,
                };
                let unit = CompiledUnit {
                    path: path.to_path_buf(),
                    stem: unit_stem(path),
                    source: source_text,
                    module: Some(module),
                    generated: Some(out.clone()),
                    diagnostics,
                    admitted: true,
                    allowed,
                    codegen_error: None,
                };
                self.memos
                    .get_mut(path)
                    .expect("lowered memo entry exists")
                    .unit = Some(unit);
                fire_stage(
                    observer,
                    Stage::Codegen,
                    path,
                    StageOutcome::Computed,
                    codegen_elapsed,
                );
                Ok(out)
            }
            Err(e) => {
                // The typed codegen seam (M18): the failure surfaces as a
                // miette `Diagnostic` (code `ligature::codegen`, help-text)
                // on the unit and as a warn event on the observability seam
                // — never a bare `String`.
                tracing::warn!(unit = %path.display(), "codegen failed: {e}");
                let failure = CodegenFailure::from(path, &e);
                let unit = CompiledUnit {
                    path: path.to_path_buf(),
                    stem: unit_stem(path),
                    source: source_text,
                    module: Some(module),
                    generated: None,
                    diagnostics,
                    admitted: true,
                    allowed,
                    codegen_error: Some(failure.clone()),
                };
                self.memos
                    .get_mut(path)
                    .expect("lowered memo entry exists")
                    .unit = Some(unit);
                fire_stage(
                    observer,
                    Stage::Codegen,
                    path,
                    StageOutcome::Failed,
                    codegen_elapsed,
                );
                Err(failure)
            }
        }
    }

    /// One full compile pass: run every query for every source, reuse memos
    /// wherever the recorded digests still validate (D-9). The unobserved
    /// form — the same pass [`Session::compile_with`] runs, with no events
    /// built and no observer driven.
    pub fn compile(&mut self, profile: Profile) -> CompileReport {
        self.compile_impl(profile, &mut None)
    }

    /// One full compile pass, observed — the session-observer seam: the same
    /// digest-gated pass [`Session::compile`] runs, with one [`StageEvent`]
    /// per stage per unit (`lower` → `admit` → `codegen`) and the pass
    /// report delivered to `observer` as they happen. See [`observe`] for
    /// the event shape and the observer contract;
    /// [`observe::TracingObserver`] forwards the events to `tracing` when a
    /// subscriber should see the per-stage numbers.
    pub fn compile_with(
        &mut self,
        profile: Profile,
        observer: &mut dyn SessionObserver,
    ) -> CompileReport {
        self.compile_impl(profile, &mut Some(observer))
    }

    /// The one compile body both entry points drive: an
    /// `Option<&mut dyn SessionObserver>` seat that stays `None` on the
    /// unobserved path (no events built) and reborrows the caller's
    /// observer per fire on the observed one.
    fn compile_impl(
        &mut self,
        profile: Profile,
        observer: &mut Option<&mut dyn SessionObserver>,
    ) -> CompileReport {
        let t0 = Instant::now();
        let mut recompiled = Vec::new();
        let mut reused = Vec::new();
        let paths = self.sources.keys().cloned().collect::<Vec<_>>();
        for path in &paths {
            let _pass_guard =
                tracing::info_span!("session_compile", unit = %path.display()).entered();
            let (text, digest) = {
                let src = &self.sources[path];
                (src.text.clone(), src.digest)
            };
            let had_lowered = self
                .memos
                .get(path)
                .and_then(|m| m.lowered.as_ref())
                .is_some_and(|m| m.source_digest == digest);
            let had_generated = self
                .memos
                .get(path)
                .and_then(|m| m.generated.as_ref())
                .is_some_and(|m| m.source_digest == digest && m.profile == profile);
            let _ = self.query_lowered(path, &text, digest, observer);
            let _ = self.query_generated(path, digest, profile, observer);
            if had_lowered && had_generated {
                reused.push(path.clone());
            } else {
                recompiled.push(path.clone());
            }
        }
        let report = CompileReport {
            revision: self.revision,
            recompiled,
            reused,
            elapsed: t0.elapsed(),
        };
        if let Some(observed) = observer.as_deref_mut() {
            observed.on_pass(&report);
        }
        report
    }
}

fn file_name(path: &Path) -> std::borrow::Cow<'_, str> {
    path.file_name().map_or("input.tsx".into(), |n| {
        std::borrow::Cow::from(n.to_string_lossy().into_owned())
    })
}

/// Fire one stage event to the observer seat, if occupied — the seam's
/// single fire point (an unoccupied seat builds no event at all).
fn fire_stage(
    observer: &mut Option<&mut dyn SessionObserver>,
    stage: Stage,
    unit: &Path,
    outcome: StageOutcome,
    elapsed: Duration,
) {
    if let Some(observed) = observer.as_deref_mut() {
        observed.on_stage(&StageEvent {
            stage,
            unit: unit.to_path_buf(),
            outcome,
            elapsed,
        });
    }
}

/// The LIG-E020 diagnostic for a component cycle: names the cycle path and
/// labels the first use site on the cycle (salsa cycle recovery → Deny).
#[must_use]
pub fn cycle_diagnostic(
    module: &ViewModule,
    cycle: &lig_reactivity::ComponentCycle,
) -> LigDiagnostic {
    let name = |c: lig_core::id::ComponentId| {
        module.components.get(c.get() as usize).map_or_else(
            || format!("component#{}", c.get()),
            |d| module.strings.get(d.name).to_string(),
        )
    };
    let mut path = cycle
        .components
        .iter()
        .copied()
        .map(&name)
        .collect::<Vec<_>>()
        .join(" → ");
    path.push_str(" → ");
    path.push_str(&name(cycle.components[0]));

    let span = cycle_span(module, &cycle.components).unwrap_or(Span::new(0, 0));
    LigDiagnostic::error("LIG-E020", format!("Cyclic component import: {path}"), span)
        .with_help("break the cycle by extracting the shared subtree into a third component")
}

/// Span of the first use site on the cycle (member A's use of the next
/// member), for the diagnostic's code frame.
fn cycle_span(module: &ViewModule, cycle: &[lig_core::id::ComponentId]) -> Option<Span> {
    let first = *cycle.first()?;
    let next = cycle.get(1).copied().unwrap_or(first);
    let decl = module.components.get(first.get() as usize)?;
    let mut stack = vec![decl.root.get() as usize];
    let mut seen: Vec<usize> = Vec::new();
    while let Some(f) = stack.pop() {
        if !seen.contains(&f) {
            seen.push(f);
            let fr = module.fragments.get(f)?;
            for &c in &fr.children {
                stack.push(c.get() as usize);
            }
            for d in &fr.dynamics {
                if let lig_ir::fragment::DynamicKind::Component(use_ir) = &d.kind
                    && use_ir.component == next
                    && d.span.1 > d.span.0
                {
                    return Some(Span::new(d.span.0, d.span.1));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{CompileReport, Session, unit_stem};
    use lig_admission::Profile;
    use miette::Diagnostic as _;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    fn counter() -> &'static str {
        lig_parse_solid::tests_support::COUNTER_TSX
    }

    fn p(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    /// A minimal capturing subscriber: records the name of every span
    /// created on the installing thread (the M16 observability seam's test
    /// instrument — no `tracing-subscriber` dependency needed).
    #[derive(Clone, Default)]
    struct SpanRecorder {
        names: Arc<Mutex<Vec<&'static str>>>,
    }

    impl SpanRecorder {
        fn recorded(&self) -> Vec<&'static str> {
            self.names.lock().expect("recorder mutex").clone()
        }
    }

    impl tracing::Subscriber for SpanRecorder {
        fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            self.names
                .lock()
                .expect("recorder mutex")
                .push(span.metadata().name());
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}
        fn event(&self, _event: &tracing::Event<'_>) {}
        fn enter(&self, _span: &tracing::span::Id) {}
        fn exit(&self, _span: &tracing::span::Id) {}
    }

    #[test]
    fn compile_passes_record_stage_spans() {
        // The observability seam: one full pass walks its stages inside
        // named spans (lower → admit → codegen, one per unit), so embedded
        // consumers can trace and time the pipeline without touching it.
        let recorder = SpanRecorder::default();
        let mut s = Session::new();
        s.set_source(p("counter.tsx"), counter());
        tracing::subscriber::with_default(recorder.clone(), || {
            let _ = s.compile(Profile::Layout);
        });
        let names = recorder.recorded();
        for stage in ["session_compile", "lower", "admit", "codegen"] {
            assert!(
                names.contains(&stage),
                "the pass records a `{stage}` span; got {names:?}"
            );
        }
    }

    #[test]
    fn first_pass_recompiles_second_pass_reuses() {
        let mut s = Session::new();
        s.set_source(p("counter.tsx"), counter());
        let r1: CompileReport = s.compile(Profile::Layout);
        assert_eq!(r1.recompiled.len(), 1, "first pass compiles");
        assert_eq!(r1.reused, Vec::<std::path::PathBuf>::new());
        assert_eq!(r1.revision, 1, "the initial set_source bumps the revision");

        let r2 = s.compile(Profile::Layout);
        assert!(r2.recompiled.is_empty(), "nothing changed");
        assert_eq!(r2.reused.len(), 1, "digest-gated reuse (D-9)");

        // A no-op set_source (same bytes) must not bump the revision nor
        // invalidate the memos.
        s.set_source(p("counter.tsx"), counter());
        assert_eq!(s.revision(), 1, "identical text is not an edit");
        let r3 = s.compile(Profile::Layout);
        assert!(r3.all_reused(), "no-op edit keeps every memo");
    }

    #[test]
    fn editing_one_source_recomputes_exactly_that_unit() {
        let mut s = Session::new();
        s.set_source(p("a.tsx"), counter());
        s.set_source(p("b.vue"), VUE_COUNTER);
        s.compile(Profile::Layout);
        let edited = counter().replace("Add", "Add2");
        assert_ne!(edited, counter());
        s.set_source(p("a.tsx"), edited);
        let r = s.compile(Profile::Layout);
        assert_eq!(r.recompiled, vec![p("a.tsx")], "only the edited unit");
        assert_eq!(r.reused, vec![p("b.vue")]);
    }

    #[test]
    fn profile_change_recompiles_the_admission_gate() {
        let mut s = Session::new();
        s.set_source(p("counter.tsx"), counter());
        s.compile(Profile::Layout);
        let r = s.compile(Profile::Web);
        assert_eq!(r.recompiled.len(), 1, "profile is part of the memo key");
    }

    #[test]
    fn removed_sources_drop_memos() {
        let mut s = Session::new();
        s.set_source(p("a.tsx"), counter());
        s.compile(Profile::Layout);
        s.remove_source(&p("a.tsx"));
        assert!(s.units().is_empty());
        assert_eq!(s.sources(), Vec::<PathBuf>::new());
    }

    #[test]
    fn denied_unit_carries_diagnostics_and_no_code() {
        // `.filter(...)` inside a view expression is a Deny-tier construct
        // (LIG-E114) — the unit must be denied with no generated output.
        const DENIED: &str = concat!(
            "import { createSignal } from \"solid-js\";\n",
            "export function App() {\n",
            "  const [items] = createSignal([1, 2, 3]);\n",
            "  return <ul>{items().filter(i => i > 1)}</ul>;\n",
            "}\n",
        );
        let mut s = Session::new();
        s.set_source(p("denied.tsx"), DENIED);
        s.compile(Profile::Layout);
        let unit = s.unit(&p("denied.tsx")).expect("unit present");
        assert!(!unit.admitted, "denied");
        assert!(unit.generated.is_none(), "BR-1: no output for denied units");
        assert!(
            unit.diagnostics.iter().any(|d| d.code == "LIG-E114"),
            "the admission diagnostic is carried: {:?}",
            unit.diagnostics
        );
    }

    #[test]
    fn cyclic_components_are_a_lig_e020_deny() {
        const CYCLE: &str = concat!(
            "export function A() {\n",
            "  return <B />;\n",
            "}\n",
            "export function B() {\n",
            "  return <A />;\n",
            "}\n",
        );
        let mut s = Session::new();
        s.set_source(p("cycle.tsx"), CYCLE);
        s.compile(Profile::Layout);
        let unit = s.unit(&p("cycle.tsx")).expect("unit present");
        assert!(!unit.admitted, "the cycle is a Deny, never a hang");
        assert!(unit.generated.is_none());
        let e020 = unit
            .diagnostics
            .iter()
            .find(|d| d.code == "LIG-E020")
            .expect("LIG-E020 present");
        assert!(e020.message.contains('A'), "names the cycle: {e020:?}");
        assert!(e020.message.contains('B'), "names the cycle: {e020:?}");
        assert!(e020.span.end > e020.span.start, "labels a use site");
    }

    #[test]
    fn stems_follow_the_fixture_convention() {
        assert_eq!(
            unit_stem(Path::new("fixtures/counter/input.solid.tsx")),
            "counter"
        );
        assert_eq!(unit_stem(Path::new("src/counter.tsx")), "counter");
        assert_eq!(unit_stem(Path::new("app.vue")), "app");
    }

    #[test]
    fn codegen_backstop_failures_are_typed_diagnostics() {
        // M18 micro-debt cleanup: the codegen seam fails as a typed
        // `CodegenFailure` — a miette `Diagnostic` with the stable code
        // `ligature::codegen` and help-text naming the fix — instead of the
        // bare `String` the pre-M18 seam carried. The red shape: a
        // state-reading bind inside per-item slot content lowers and admits
        // (the frontend contract holds), then the codegen backstop fails.
        const STATE_FALLBACK: &str = concat!(
            "import { createSignal, For } from \"solid-js\";\n",
            "export function Inner() {\n",
            "  const [label, setLabel] = createSignal(\"static\");\n",
            "  const [items, setItems] = createSignal([\"alpha\"]);\n",
            "  return (\n",
            "    <ul>\n",
            "      <For each={items()} key={(r) => r}>\n",
            "        {(r) => <Slot name=\"row\"><i class=\"row\">{label}</i></Slot>}\n",
            "      </For>\n",
            "    </ul>\n",
            "  );\n",
            "}\n",
        );
        let mut s = Session::new();
        s.set_source(p("state_fallback.tsx"), STATE_FALLBACK);
        s.compile(Profile::Layout);
        let unit = s.unit(&p("state_fallback.tsx")).expect("unit present");
        let err = unit
            .codegen_error
            .as_ref()
            .expect("the codegen backstop denies the state-reading bind");
        assert!(
            err.code().map(|c| c.to_string()).as_deref() == Some("ligature::codegen"),
            "a stable code: {err:?}"
        );
        let help = err
            .help()
            .map(|h| h.to_string())
            .expect("miette help-text present");
        assert!(
            help.contains("item") && help.contains("keyed component"),
            "the help names the per-item discipline and the escape: {help}"
        );
        assert!(
            err.to_string().contains("per-item slot content"),
            "the precise backstop reason renders as the message: {err}"
        );
        assert!(unit.generated.is_none(), "a failed unit carries no output");
    }

    #[test]
    fn unsupported_extensions_deny_with_one_precise_reason() {
        // M18 micro-debt cleanup: `lower_source` used to fall back to the
        // Solid frontend for ANY non-.vue extension, so a `.js` file (or a
        // typo'd `.tssx`) produced confusing syntax errors far from the
        // cause. The seam now fails with one precise diagnostic naming the
        // two supported extensions.
        let diags = super::lower_source("const x = 1;\n", "script.js").expect_err("denied");
        assert_eq!(diags.len(), 1, "one precise reason: {diags:?}");
        let d = &diags[0];
        assert_eq!(d.code, "LIG-E001", "the lowering-contract code");
        assert!(
            d.message.contains(".tsx") && d.message.contains(".vue"),
            "the reason names the supported extensions: {}",
            d.message
        );
        // Both supported extensions still lower (the dispatch is unchanged).
        assert!(super::lower_source(counter(), "counter.tsx").is_ok());
        let vue_counter = include_str!("../../../fixtures/conformance/counter/input.vue");
        assert!(super::lower_source(vue_counter, "counter.vue").is_ok());
    }

    const VUE_COUNTER: &str = concat!(
        "<script setup>\n",
        "import { ref, computed } from \"vue\";\n",
        "const count = ref(0);\n",
        "const doubled = computed(() => count.value * 2);\n",
        "function inc() {\n",
        "  count.value = count.value + 1;\n",
        "}\n",
        "defineOptions({ name: \"counter\" });\n",
        "</script>\n",
        "<template>\n",
        "  <button class=\"btn\" onClick={inc}>\n",
        "    {count()} + {doubled()}\n",
        "  </button>\n",
        "</template>\n",
    );

    #[test]
    fn sync_reports_added_edited_removed() {
        // The watch-sync seam: one call diffs a fresh source snapshot
        // against the session — additions and edits register (the edit
        // bumps the revision, the addition appears), an unchanged entry is
        // a no-op, and a registered source absent from the snapshot is
        // dropped with its memos.
        let mut s = Session::new();
        s.set_source(p("a.tsx"), counter());
        s.set_source(p("b.tsx"), counter());
        s.set_source(p("d.tsx"), counter());
        let edited = counter().replace("Add", "Add2");
        let report = s.sync(
            vec![
                (p("a.tsx"), edited.as_str()), // edit
                (p("c.vue"), VUE_COUNTER),     // addition
                (p("b.tsx"), counter()),       // unchanged — no-op
                                               // d.tsx is absent — removed
            ],
            &[],
        );
        assert_eq!(report.added, vec![p("c.vue")]);
        assert_eq!(report.edited, vec![p("a.tsx")]);
        assert_eq!(report.removed, vec![p("d.tsx")]);
        assert_eq!(s.sources(), vec![p("a.tsx"), p("b.tsx"), p("c.vue")]);
        assert_eq!(s.revision(), 6, "initial 3 + edit + addition + removal");
    }

    #[test]
    fn sync_is_a_noop_on_an_identical_snapshot() {
        // The no-op-edit discipline (D-9) holds through sync: an identical
        // snapshot registers nothing, bumps no revision, keeps every memo.
        let mut s = Session::new();
        s.sync(
            vec![(p("a.tsx"), counter()), (p("b.vue"), VUE_COUNTER)],
            &[],
        );
        s.compile(Profile::Layout);
        let rev = s.revision();
        let report = s.sync(
            vec![(p("a.tsx"), counter()), (p("b.vue"), VUE_COUNTER)],
            &[],
        );
        assert!(report.is_empty(), "identical snapshot: {report:?}");
        assert_eq!(s.revision(), rev, "no phantom edits");
        let r = s.compile(Profile::Layout);
        assert!(r.all_reused(), "memos survive the no-op sync");
    }

    #[test]
    fn sync_keeps_explicit_roots_not_in_the_snapshot() {
        // The CLI watch passes its explicit root paths as `keep`: a root
        // that vanished from disk (or failed to read) keeps its last
        // registered text — the pre-seam watch-loop discipline — while a
        // scanned source absent from the snapshot leaves the session.
        let mut s = Session::new();
        s.sync(
            vec![(p("root.tsx"), counter()), (p("scan.tsx"), counter())],
            &[],
        );
        let report = s.sync(vec![(p("root.tsx"), counter())], &[p("root.tsx")]);
        assert_eq!(
            report.removed,
            vec![p("scan.tsx")],
            "the absent scan leaves"
        );
        assert!(s.sources().contains(&p("root.tsx")), "the kept root stays");
        // Without keep, the same vanished root leaves too.
        let report = s.sync(Vec::<(std::path::PathBuf, &str)>::new(), &[]);
        assert_eq!(report.removed, vec![p("root.tsx")]);
        assert_eq!(
            s.sources(),
            Vec::<std::path::PathBuf>::new(),
            "without keep the vanished root leaves too"
        );
    }

    #[test]
    fn sync_then_compile_recompiles_exactly_the_synced_units() {
        // The engine story end to end: sync feeds the digest gate — only
        // the added and edited units recompute; unchanged units reuse.
        let mut s = Session::new();
        s.set_source(p("a.tsx"), counter());
        s.set_source(p("b.vue"), VUE_COUNTER);
        s.compile(Profile::Layout);
        let edited = counter().replace("Add", "Add2");
        let report = s.sync(
            vec![
                (p("a.tsx"), edited.as_str()),
                (p("b.vue"), VUE_COUNTER),
                (p("c.tsx"), counter()),
            ],
            &[],
        );
        let r = s.compile(Profile::Layout);
        assert_eq!(r.recompiled.len(), report.added.len() + report.edited.len());
        assert_eq!(r.reused, vec![p("b.vue")], "the untouched unit is reused");
    }

    #[test]
    fn source_text_reads_back_the_registered_text() {
        // The session's own input accessor: watch integrations re-feed the
        // last known text for a file that still exists but failed to read
        // (the pre-seam loop kept it registered with stale text — the
        // seam preserves that through the accessor).
        let mut s = Session::new();
        assert_eq!(s.source_text(&p("missing.tsx")), None);
        s.set_source(p("a.tsx"), counter());
        assert_eq!(s.source_text(&p("a.tsx")), Some(counter()));
    }
}
