//! The session-observer seam (M19): structured, programmatic visibility into
//! the digest-gated compile pipeline. The M16 `tracing` spans name the stages
//! (`session_compile` wrapping `lower` → `admit` → `codegen` per unit); this
//! seam delivers the same stages as **events to a caller-owned observer**, so
//! an embedded host, an IDE plugin, or a playground gets per-stage wall clocks
//! and outcomes without installing a log subscriber or scraping span records.
//!
//! Shape: [`Session::compile_with`] runs one pass through a
//! `&mut dyn SessionObserver` — the same object-safe seam shape the pipeline
//! already speaks (`&mut dyn Host`, `&mut dyn ArtifactSink`). Every stage of
//! every unit reports one [`StageEvent`] — `Computed` with the stage's wall
//! clock, `Failed` when the stage denied or the backstop fired, `Reused`
//! (zero elapsed) when the D-9 memo still validated — and one
//! [`SessionObserver::on_pass`] with the same [`CompileReport`] `compile`
//! returns. Stages that never run (codegen after an admission denial, admit
//! after a lowering failure) report no event: the seam never invents stages.
//! Plain [`Session::compile`] is this seam with a private no-op observer.
//!
//! Contract: an observer MUST NOT panic (a panicking observer unwinds the
//! compile) and MUST NOT block (it rides the pipeline's critical path); it
//! receives borrow-only views, so persisting an event is the implementation's
//! clone. The observer state stays caller-owned: it is readable the moment
//! `compile_with` returns.

use crate::CompileReport;
use std::path::PathBuf;
use std::time::Duration;

/// A pipeline stage — 1:1 with the M16 `tracing` span names, so a trace and
/// an observer event stream interleave into the same story.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// Parse + lower through the frontend the extension selects (the `lower`
    /// span).
    Lower,
    /// Admission tiering + the build-order cycle check (LIG-E020) — the
    /// `admit` span.
    Admit,
    /// The `quote!` → `syn` → `prettyplease` codegen (D-8) — the `codegen`
    /// span.
    Codegen,
}

impl Stage {
    /// The M16 span name the stage rides (`lower`, `admit`, `codegen`) —
    /// the field a trace and an event share.
    #[must_use]
    pub fn span_name(self) -> &'static str {
        match self {
            Self::Lower => "lower",
            Self::Admit => "admit",
            Self::Codegen => "codegen",
        }
    }
}

/// What one stage produced for one unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageOutcome {
    /// The stage recomputed and passed.
    Computed,
    /// The stage recomputed and failed — a lowering error, an admission
    /// denial (or cycle, LIG-E020), or the codegen backstop. The details
    /// ride the unit's diagnostics: [`crate::Session::unit`].
    Failed,
    /// The stage's memo still validated (digest + profile unchanged, D-9) —
    /// nothing recomputed, zero elapsed. A reused memo reports `Reused`
    /// whatever outcome it recorded; the unit carries the details.
    Reused,
}

/// One stage event: which stage, for which unit, with what outcome, and the
/// stage's wall clock (zero for a reused memo).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageEvent {
    /// The stage that produced the event.
    pub stage: Stage,
    /// The unit the stage ran for (the registered source path).
    pub unit: PathBuf,
    /// What the stage produced.
    pub outcome: StageOutcome,
    /// The stage's wall clock — zero for a reused memo.
    pub elapsed: Duration,
}

/// The session-observer seam: a caller-owned sink for the pipeline's stage
/// events. Every method defaults to a no-op so an implementor picks exactly
/// what it wants; the trait is object-safe (driven as
/// `&mut dyn SessionObserver`) and synchronously driven on the compiling
/// thread — an observer sees the pass in order, never concurrently.
pub trait SessionObserver {
    /// One stage of one unit finished. Fired in pipeline order (per unit:
    /// `lower` → `admit` → `codegen`; a stage that never runs reports
    /// nothing).
    fn on_stage(&mut self, _event: &StageEvent) {}

    /// One full pass finished — the same [`CompileReport`]
    /// [`Session::compile_with`] returns, delivered before it does.
    fn on_pass(&mut self, _report: &CompileReport) {}
}

/// The tracing-forwarding observer: re-emits each stage event as a flat
/// `tracing::info!` event (`session_stage`) with the stage's span name, the
/// unit, the outcome, and the elapsed time in µs as fields — the durations
/// the spans alone do not record. Install it when a subscriber should see
/// filterable per-stage numbers; write your own [`SessionObserver`] when
/// the numbers should feed code instead.
#[derive(Debug, Default, Clone, Copy)]
pub struct TracingObserver;

impl SessionObserver for TracingObserver {
    fn on_stage(&mut self, event: &StageEvent) {
        tracing::info!(
            stage = event.stage.span_name(),
            unit = %event.unit.display(),
            outcome = ?event.outcome,
            elapsed_us = event.elapsed.as_micros() as u64,
            "session_stage"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionObserver, Stage, StageEvent, StageOutcome, TracingObserver};
    use crate::{CompileReport, Session};
    use lig_admission::Profile;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn p(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    fn counter() -> &'static str {
        lig_parse_solid::tests_support::COUNTER_TSX
    }

    /// The M18 codegen-backstop workload (the lib.rs test's source): a
    /// state-reading bind inside per-item slot content lowers and admits,
    /// then the codegen backstop fails — the `Codegen` stage reports
    /// `Failed`.
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

    /// The LIG-E117 denial workload (raw HTML on the layout-only profile):
    /// the source LOWERS fine, then the admit stage fails — codegen never
    /// runs.
    const DENIED: &str = concat!(
        "import { createSignal } from \"solid-js\";\n",
        "export function A() {\n",
        "  const [h, setH] = createSignal(\"<b>x</b>\");\n",
        "  return <div innerHTML={h()} />;\n",
        "}\n",
    );

    /// A parse error: the `lower` stage fails before any other stage exists.
    const PARSE_ERROR: &str = "export function App( {\n";

    /// The recording observer the tests drive: stage events in order, plus
    /// the pass reports. State stays caller-owned — readable the moment the
    /// pass returns.
    #[derive(Default)]
    struct Recording {
        events: Vec<(Stage, StageOutcome)>,
        passes: Vec<CompileReport>,
    }

    impl SessionObserver for Recording {
        fn on_stage(&mut self, event: &StageEvent) {
            self.events.push((event.stage, event.outcome));
        }

        fn on_pass(&mut self, report: &CompileReport) {
            self.passes.push(report.clone());
        }
    }

    /// A minimal capturing subscriber: records the field set of every event
    /// on the installing thread (the `TracingObserver`'s test instrument —
    /// the same shape the span-recorder test in lib.rs drives).
    #[derive(Clone, Default)]
    struct EventRecorder {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl EventRecorder {
        fn recorded(&self) -> Vec<String> {
            self.events.lock().expect("recorder mutex").clone()
        }
    }

    impl tracing::Subscriber for EventRecorder {
        fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}
        fn enter(&self, _span: &tracing::span::Id) {}
        fn exit(&self, _span: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            let lines = Arc::new(Mutex::new(Vec::new()));
            event.record(&mut FieldLines(lines.clone()));
            self.events
                .lock()
                .expect("recorder mutex")
                .push(lines.lock().expect("field mutex").join(" "));
        }
    }

    struct FieldLines(Arc<Mutex<Vec<String>>>);

    impl tracing::field::Visit for FieldLines {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn core::fmt::Debug) {
            self.0
                .lock()
                .expect("field mutex")
                .push(format!("{}={:?}", field.name(), value));
        }
    }

    #[test]
    fn stages_map_one_to_one_onto_the_m16_spans() {
        assert_eq!(Stage::Lower.span_name(), "lower");
        assert_eq!(Stage::Admit.span_name(), "admit");
        assert_eq!(Stage::Codegen.span_name(), "codegen");
    }

    #[test]
    fn observer_walks_the_cold_pass_stages_in_order() {
        // One cold pass over one unit: exactly the three M16 stages, in
        // pipeline order, each Computed, and the pass rides `on_pass`.
        let mut s = Session::new();
        s.set_source(p("counter.tsx"), counter());
        let mut rec = Recording::default();
        let report = s.compile_with(Profile::Layout, &mut rec);
        assert_eq!(report.recompiled.len(), 1, "a cold pass compiles");
        assert_eq!(
            rec.events,
            vec![
                (Stage::Lower, StageOutcome::Computed),
                (Stage::Admit, StageOutcome::Computed),
                (Stage::Codegen, StageOutcome::Computed),
            ],
            "the pipeline order, stage by stage: {:?}",
            rec.events
        );
        assert_eq!(rec.passes.len(), 1, "one pass, one on_pass");
        assert_eq!(rec.passes[0].recompiled, vec![p("counter.tsx")]);
    }

    #[test]
    fn observer_sees_only_reuse_on_a_warm_pass() {
        // The D-9 half: the second pass reuses every memo — every stage
        // reports `Reused` with zero elapsed (the seam's "nothing ran"
        // signal), and `on_pass` still fires.
        let mut s = Session::new();
        s.set_source(p("counter.tsx"), counter());
        s.compile(Profile::Layout);
        let mut rec = Recording::default();
        let report = s.compile_with(Profile::Layout, &mut rec);
        assert!(report.all_reused(), "the warm pass reuses");
        assert_eq!(
            rec.events,
            vec![
                (Stage::Lower, StageOutcome::Reused),
                (Stage::Admit, StageOutcome::Reused),
                (Stage::Codegen, StageOutcome::Reused),
            ],
            "every stage is a memo hit: {:?}",
            rec.events
        );
        assert_eq!(rec.passes.len(), 1, "the warm pass still reports");
    }

    #[test]
    fn observer_marks_the_admit_stage_failed_on_a_denial() {
        // A Deny-tier construct (LIG-E117: raw HTML on the layout-only
        // profile): the admit stage recomputes and fails; codegen never runs
        // (no event — the seam never invents stages), and the denial details
        // stay on the unit.
        let mut s = Session::new();
        s.set_source(p("denied.tsx"), DENIED);
        let mut rec = Recording::default();
        let _ = s.compile_with(Profile::Layout, &mut rec);
        assert_eq!(
            rec.events,
            vec![
                (Stage::Lower, StageOutcome::Computed),
                (Stage::Admit, StageOutcome::Failed),
            ],
            "admit fails, codegen never runs: {:?}",
            rec.events
        );
        let unit = s.unit(&p("denied.tsx")).expect("unit present");
        assert!(!unit.admitted);
        assert!(
            unit.diagnostics.iter().any(|d| d.code == "LIG-E117"),
            "the denial rides the unit as always"
        );
    }

    #[test]
    fn observer_marks_the_lower_stage_failed_on_a_parse_error() {
        // A parse error fails the FIRST stage; nothing else runs.
        let mut s = Session::new();
        s.set_source(p("broken.tsx"), PARSE_ERROR);
        let mut rec = Recording::default();
        let _ = s.compile_with(Profile::Layout, &mut rec);
        assert_eq!(
            rec.events,
            vec![(Stage::Lower, StageOutcome::Failed)],
            "lower fails, nothing downstream: {:?}",
            rec.events
        );
    }

    #[test]
    fn observer_marks_the_codegen_stage_failed_on_the_backstop() {
        // The M18 typed codegen seam, observed: the backstop fails the
        // codegen stage after lower and admit passed.
        let mut s = Session::new();
        s.set_source(p("state_fallback.tsx"), STATE_FALLBACK);
        let mut rec = Recording::default();
        let _ = s.compile_with(Profile::Layout, &mut rec);
        assert_eq!(
            rec.events,
            vec![
                (Stage::Lower, StageOutcome::Computed),
                (Stage::Admit, StageOutcome::Computed),
                (Stage::Codegen, StageOutcome::Failed),
            ],
            "the backstop fails exactly the codegen stage: {:?}",
            rec.events
        );
        let unit = s.unit(&p("state_fallback.tsx")).expect("unit present");
        assert!(unit.codegen_error.is_some(), "the failure rides the unit");
    }

    #[test]
    fn stage_events_carry_the_stage_wall_clock() {
        // Computed events carry the stage's own wall clock (the codegen
        // stage — quote! → syn → prettyplease — dominates the lower stage on
        // the counter workload); Reused events carry zero.
        #[derive(Default)]
        struct Timing {
            elapsed: Vec<(Stage, StageOutcome, Duration)>,
        }
        impl SessionObserver for Timing {
            fn on_stage(&mut self, event: &StageEvent) {
                self.elapsed
                    .push((event.stage, event.outcome, event.elapsed));
            }
        }
        let mut s = Session::new();
        s.set_source(p("counter.tsx"), counter());
        let mut timing = Timing::default();
        let _ = s.compile_with(Profile::Layout, &mut timing);
        let _ = s.compile_with(Profile::Layout, &mut timing);
        let (cold, warm) = timing.elapsed.split_at(3);
        assert!(
            cold.iter().all(|(_, o, _)| *o == StageOutcome::Computed),
            "the cold pass computes: {cold:?}"
        );
        assert!(
            warm.iter()
                .all(|(_, o, t)| *o == StageOutcome::Reused && *t == Duration::ZERO),
            "the warm pass reuses at zero elapsed: {warm:?}"
        );
        let codegen = cold
            .iter()
            .find(|(s, _, _)| *s == Stage::Codegen)
            .map(|(_, _, t)| *t)
            .expect("a codegen row");
        let lower = cold
            .iter()
            .find(|(s, _, _)| *s == Stage::Lower)
            .map(|(_, _, t)| *t)
            .expect("a lower row");
        assert!(
            codegen > lower,
            "codegen ({codegen:?}) dominates lower ({lower:?}) on the counter"
        );
    }

    #[test]
    fn on_pass_receives_the_pass_report_before_compile_returns_it() {
        // `on_pass` hands over the SAME report the call returns — the
        // observer is a preview, never a second source of truth.
        #[derive(Default)]
        struct LastPass {
            revision: Option<u64>,
        }
        impl SessionObserver for LastPass {
            fn on_pass(&mut self, report: &CompileReport) {
                self.revision = Some(report.revision);
            }
        }
        let mut s = Session::new();
        s.set_source(p("counter.tsx"), counter());
        let mut observer = LastPass::default();
        let report = s.compile_with(Profile::Layout, &mut observer);
        assert_eq!(
            observer.revision,
            Some(report.revision),
            "the observer saw the returned report"
        );
    }

    #[test]
    fn a_session_without_an_observer_is_unchanged() {
        // The no-observer path is byte-for-byte the pre-seam behavior: the
        // same report, the same units, nothing else.
        let mut s = Session::new();
        s.set_source(p("counter.tsx"), counter());
        let report = s.compile(Profile::Layout);
        assert_eq!(report.recompiled.len(), 1);
        assert!(s.unit(&p("counter.tsx")).expect("unit").admitted);
    }

    #[test]
    fn tracing_observer_reports_stage_events_with_fields() {
        // The shipped forwarding impl: one flat event per stage with the
        // stage's span name, the unit, the outcome, and the µs elapsed as
        // filterable fields.
        let recorder = EventRecorder::default();
        let mut s = Session::new();
        s.set_source(p("counter.tsx"), counter());
        tracing::subscriber::with_default(recorder.clone(), || {
            let mut observer = TracingObserver;
            let _ = s.compile_with(Profile::Layout, &mut observer);
        });
        let events = recorder.recorded();
        assert!(events.len() >= 3, "one event per stage: {}", events.len());
        let joined = events.join("\n");
        for needle in ["stage=\"lower\"", "stage=\"admit\"", "stage=\"codegen\""] {
            assert!(joined.contains(needle), "the stage field: {needle}");
        }
        assert!(joined.contains("unit=counter.tsx"), "the unit field");
        assert!(joined.contains("Computed"), "the outcome field");
        assert!(joined.contains("elapsed_us="), "the elapsed field");
        assert!(joined.contains("session_stage"), "the event name");
    }
}
