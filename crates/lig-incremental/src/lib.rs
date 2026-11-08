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

use lig_admission::{Profile, admit};
use lig_codegen::{GenLinks, generate_module};
use lig_core::diagnostic::LigDiagnostic;
use lig_core::hash::fnv1a64;
use lig_core::span::Span;
use lig_ir::ViewModule;
use lig_reactivity::component_build_order;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The FNV-1a 64 digest that gates every memo (D-9's currency).
#[must_use]
pub fn digest(bytes: &[u8]) -> u64 {
    fnv1a64(bytes)
}

/// Lower a source file with the frontend its extension selects (§6: `.tsx`
/// → Solid, `.vue` → Vue SFC). The canonical IR is the same contract.
pub fn lower_source(
    source: &str,
    name: &str,
) -> Result<ViewModule, Vec<lig_core::diagnostic::LigDiagnostic>> {
    if Path::new(name)
        .extension()
        .is_some_and(|x| x.eq_ignore_ascii_case("vue"))
    {
        lig_parse_vue::lower_module(source, name)
    } else {
        lig_parse_solid::lower_module(source, name)
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
    pub codegen_error: Option<String>,
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

/// A incremental compilation session: inputs + memos + revision counter.
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
    /// selects, memoized on the source digest.
    fn query_lowered(&mut self, path: &Path, source: &str, source_digest: u64) -> &LoweredMemo {
        let valid = self
            .memos
            .get(path)
            .and_then(|m| m.lowered.as_ref())
            .is_some_and(|m| m.source_digest == source_digest);
        if !valid {
            let outcome = lower_source(source, &file_name(path));
            let memo = UnitMemo {
                lowered: Some(LoweredMemo {
                    source_digest,
                    outcome,
                }),
                generated: None,
                unit: None,
            };
            self.memos.insert(path.to_path_buf(), memo);
        }
        self.memos
            .get_mut(path)
            .and_then(|m| m.lowered.as_ref())
            .expect("memo just validated or inserted")
    }

    /// The `generated` query: admission (profile-dependent) → build-order
    /// cycle check (LIG-E020) → codegen, memoized on source digest + profile.
    fn query_generated(
        &mut self,
        path: &Path,
        source_digest: u64,
        profile: Profile,
    ) -> &GeneratedMemo {
        let valid = self
            .memos
            .get(path)
            .and_then(|m| m.generated.as_ref())
            .is_some_and(|m| m.source_digest == source_digest && m.profile == profile);
        if !valid {
            let outcome = self.compute_generated(path, profile);
            let entry = self
                .memos
                .entry(path.to_path_buf())
                .or_insert_with(UnitMemo::empty);
            let _ = outcome;
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
    /// the resulting [`CompiledUnit`] on the memo entry.
    fn compute_generated(
        &mut self,
        path: &Path,
        profile: Profile,
    ) -> Result<GeneratedOutput, String> {
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

        // Stage 2: admission (profile tiering).
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
            return Ok(GeneratedOutput::denied());
        }

        // Stage 4: codegen (D-8 byte-reproducible).
        match generate_module(&module) {
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
                Ok(out)
            }
            Err(e) => {
                let msg = e.to_string();
                let unit = CompiledUnit {
                    path: path.to_path_buf(),
                    stem: unit_stem(path),
                    source: source_text,
                    module: Some(module),
                    generated: None,
                    diagnostics,
                    admitted: true,
                    allowed,
                    codegen_error: Some(msg.clone()),
                };
                self.memos
                    .get_mut(path)
                    .expect("lowered memo entry exists")
                    .unit = Some(unit);
                Err(msg)
            }
        }
    }

    /// One full compile pass: run every query for every source, reuse memos
    /// wherever the recorded digests still validate (D-9).
    pub fn compile(&mut self, profile: Profile) -> CompileReport {
        let t0 = Instant::now();
        let mut recompiled = Vec::new();
        let mut reused = Vec::new();
        let paths = self.sources.keys().cloned().collect::<Vec<_>>();
        for path in &paths {
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
            let _ = self.query_lowered(path, &text, digest);
            let _ = self.query_generated(path, digest, profile);
            if had_lowered && had_generated {
                reused.push(path.clone());
            } else {
                recompiled.push(path.clone());
            }
        }
        CompileReport {
            revision: self.revision,
            recompiled,
            reused,
            elapsed: t0.elapsed(),
        }
    }
}

fn file_name(path: &Path) -> std::borrow::Cow<'_, str> {
    path.file_name().map_or("input.tsx".into(), |n| {
        std::borrow::Cow::from(n.to_string_lossy().into_owned())
    })
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
    use std::path::{Path, PathBuf};

    fn counter() -> &'static str {
        lig_parse_solid::tests_support::COUNTER_TSX
    }

    fn p(name: &str) -> PathBuf {
        PathBuf::from(name)
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
}
