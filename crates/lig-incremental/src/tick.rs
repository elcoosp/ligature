//! The watch-tick seam (M23): `lig watch`'s per-save loop — snapshot diff →
//! digest-gated compile → fail-closed emission — as one session call.
//!
//! Before this seam the tick was caller-assembled three ways at once: the
//! CLI loop body ran [`Session::sync`], then [`Session::compile`], then the
//! deny check and [`Session::emit_to`] by hand, and every embedded consumer
//! that wanted watch-like live reload had to re-assemble the same four
//! steps — and get the BR-1 ordering right on their own (a denied pass must
//! never touch the sink). This module makes the tick one surface, the same
//! move the mount seam (M20) made for the runtime half's build → flush →
//! dispose loop:
//!
//! - [`Session::tick`] diffs the fresh snapshot into the session (the
//!   watch-sync seam), compiles exactly what the digest gate invalidates
//!   (D-9), gates the pass fail-closed (BR-1: a denied or codegen-failed
//!   unit refuses the WHOLE emission — the sink never sees a byte), and
//!   writes the §9.5 artifact set into any [`ArtifactSink`] with the digest
//!   gating and the profile ledger name the seams already speak;
//! - [`Session::tick_with`] drives the same pass through a
//!   [`SessionObserver`] — one [`StageEvent`] per stage per unit, 1:1 with
//!   the `tracing` spans (M16), plus the pass report — the observability
//!   convention every session seam follows.
//!
//! The report is the whole tick: [`TickReport::sync`] (what the snapshot
//! changed), [`TickReport::pass`] (what recomputed vs reused), and
//! [`TickReport::emission`] (what wrote, kept, and pruned). On the BR-1
//! refusal the error is the typed [`EmitError`] the emission seam already
//! returns — the sync and the compile have already applied to the session
//! (inspect the units for the diagnostics), and the sink saw nothing.
//!
//! Shape: no new trait — the tick composes the seams the session already
//! speaks ([`Session::sync`], the digest-gated pass, [`ArtifactSink`]) the
//! way [`Session::emit_to`] composes the emission; the sink stays
//! object-safe (`&mut dyn ArtifactSink`) and the observer stays
//! caller-owned, readable the moment the call returns. One deliberate
//! divergence from [`Session::emit_to`], pinned by a test: an EMPTIED
//! session (every source removed by the snapshot) is a legitimate watch
//! state — the tick writes the empty manifest and prunes the leftovers,
//! where `emit_to` refuses an empty session because there the empty set
//! means "forgot to compile".

use crate::emit::{ArtifactSink, EmissionReport, EmitError, emit_units_to};
use crate::observe::SessionObserver;
use crate::{CompileReport, Session, SyncReport};
use lig_admission::Profile;
use std::path::PathBuf;

/// One watch tick, end to end: what the snapshot changed, what the pass
/// recomputed, and what the emission wrote — the three reports the `lig
/// watch` loop prints, delivered as one value.
#[derive(Debug, Clone)]
pub struct TickReport {
    /// The snapshot diff ([`Session::sync`]): added, edited, removed paths.
    pub sync: SyncReport,
    /// The digest-gated compile pass ([`Session::compile`]): recompiled,
    /// reused, revision, wall clock.
    pub pass: CompileReport,
    /// The digest-gated emission: written, kept, pruned stems and the
    /// per-stem sourcemap verification stats.
    pub emission: EmissionReport,
}

impl Session {
    /// One watch tick (M23): diff the fresh source snapshot into the
    /// session, compile what the digest gate invalidated, and emit the
    /// artifact set into `sink` — the loop body `lig watch` runs per save,
    /// as one call.
    ///
    /// The composition order is the fail-closed contract: the sync applies
    /// first (additions/edits register with the D-9 no-op discipline,
    /// removals drop with their memos unless kept), the compile runs
    /// digest-gated, then the emission is gated BR-1 — a session holding a
    /// denied unit or a codegen-backstop failure refuses the WHOLE emission
    /// (nothing writes, the typed [`EmitError`] names the blocking stem
    /// with the fix), exactly like [`Session::emit_to`]. An EMPTIED session
    /// (the snapshot removed every source) is the one legitimate empty
    /// pass: the tick writes the empty manifest and prunes the leftover
    /// artifacts, the watch contract the CLI holds.
    ///
    /// `prune` rides through to the emission: artifacts of stems that left
    /// the session are removed from the sink (best-effort by the sink
    /// contract).
    ///
    /// # Errors
    /// A typed [`EmitError`] (`code` `ligature::emit`, help-text naming the
    /// fix) when the pass is not fully admitted (BR-1) or a sink write
    /// fails. The sync and the compile have already applied — inspect the
    /// session's units for the diagnostics; the sink saw no byte.
    pub fn tick<I, S>(
        &mut self,
        fresh: I,
        keep: &[PathBuf],
        sink: &mut dyn ArtifactSink,
        profile: Profile,
        prune: bool,
    ) -> Result<TickReport, EmitError>
    where
        I: IntoIterator<Item = (PathBuf, S)>,
        S: Into<String>,
    {
        self.tick_impl(fresh, keep, sink, profile, prune, &mut None)
    }

    /// The observed tick: the same pass [`Session::tick`] runs, with every
    /// stage of every unit delivered to `observer` as a [`StageEvent`] —
    /// `Computed` with the stage's wall clock, `Failed` on a denial or the
    /// backstop, `Reused` at zero on a memo hit — and the pass report
    /// delivered to [`SessionObserver::on_pass`] as it finishes. 1:1 with
    /// the M16 `tracing` spans, the same contract [`Session::compile_with`]
    /// holds.
    ///
    /// # Errors
    /// The same typed [`EmitError`] [`Session::tick`] reports; the observer
    /// has already seen the failing stage's event by then.
    pub fn tick_with<I, S>(
        &mut self,
        fresh: I,
        keep: &[PathBuf],
        sink: &mut dyn ArtifactSink,
        profile: Profile,
        prune: bool,
        observer: &mut dyn SessionObserver,
    ) -> Result<TickReport, EmitError>
    where
        I: IntoIterator<Item = (PathBuf, S)>,
        S: Into<String>,
    {
        self.tick_impl(fresh, keep, sink, profile, prune, &mut Some(observer))
    }

    /// The one tick body both entry points drive: the observer seat stays
    /// `None` on the unobserved path and reborrows the caller's observer
    /// per fire on the observed one — the same seat discipline
    /// `compile_impl` holds.
    fn tick_impl<I, S>(
        &mut self,
        fresh: I,
        keep: &[PathBuf],
        sink: &mut dyn ArtifactSink,
        profile: Profile,
        prune: bool,
        observer: &mut Option<&mut dyn SessionObserver>,
    ) -> Result<TickReport, EmitError>
    where
        I: IntoIterator<Item = (PathBuf, S)>,
        S: Into<String>,
    {
        let sync = self.sync(fresh, keep);
        let pass = self.compile_impl(profile, observer);
        let units = self.units();
        crate::emit::refuse_blocked_pass(&units)?;
        let emission = emit_units_to(sink, &units, profile.name(), prune)?;
        Ok(TickReport {
            sync,
            pass,
            emission,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::TickReport;
    use crate::emit::MemSink;
    use crate::observe::{SessionObserver, Stage, StageOutcome};
    use crate::{Session, SyncReport};
    use lig_admission::Profile;
    use lig_parse_solid::tests_support::COUNTER_TSX;
    use miette::Diagnostic as _;
    use std::path::{Path, PathBuf};

    fn p(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    /// The LIG-E117 denial workload: lowers fine, then the admit stage
    /// denies on the layout-only profile — the tick's BR-1 refusal.
    const DENIED: &str = concat!(
        "import { createSignal } from \"solid-js\";\n",
        "export function A() {\n",
        "  const [h, setH] = createSignal(\"<b>x</b>\");\n",
        "  return <div innerHTML={h()} />;\n",
        "}\n",
    );

    /// The M18 codegen-backstop workload: lowers and admits, then the
    /// backstop fails — the other tick refusal.
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

    /// The five artifact paths one counter emission lands, in the §9.5 layout.
    fn artifact_paths() -> [String; 5] {
        [
            "counter.rs".to_string(),
            "ir/counter.json".to_string(),
            "maps/counter.map.json".to_string(),
            "manifest.toml".to_string(),
            "build-state.json".to_string(),
        ]
    }

    #[test]
    fn tick_composes_sync_compile_emit_into_one_call() {
        // The cold tick: the snapshot ADDS the source, the pass compiles
        // it, the emission writes it — three reports, one call, and the
        // sink holds the full §9.5 artifact set.
        let mut s = Session::new();
        let mut sink = MemSink::new();
        let tick: TickReport = s
            .tick(
                vec![(p("counter.tsx"), COUNTER_TSX)],
                &[],
                &mut sink,
                Profile::Layout,
                false,
            )
            .expect("the cold tick passes");
        assert_eq!(tick.sync.added, vec![p("counter.tsx")], "the snapshot adds");
        assert_eq!(
            tick.pass.recompiled,
            vec![p("counter.tsx")],
            "the pass compiles"
        );
        assert_eq!(
            tick.emission.written,
            vec!["counter".to_string()],
            "the emission writes"
        );
        for path in artifact_paths() {
            let bytes = sink
                .get(&path)
                .unwrap_or_else(|| panic!("{path} emitted by the tick"));
            assert!(!bytes.is_empty(), "{path} is non-empty");
        }
    }

    #[test]
    fn tick_is_all_reuse_on_an_identical_snapshot() {
        // The no-op save: an identical snapshot registers nothing, the
        // digest gate keeps every memo, and the emission keeps every
        // artifact — the whole tick is a no-op against the sink.
        let mut s = Session::new();
        let mut sink = MemSink::new();
        s.tick(
            vec![(p("counter.tsx"), COUNTER_TSX)],
            &[],
            &mut sink,
            Profile::Layout,
            false,
        )
        .expect("the cold tick");
        let settled = sink.paths().len();
        let tick = s
            .tick(
                vec![(p("counter.tsx"), COUNTER_TSX)],
                &[],
                &mut sink,
                Profile::Layout,
                false,
            )
            .expect("the warm tick");
        assert!(tick.sync.is_empty(), "no snapshot change: {:?}", tick.sync);
        assert!(tick.pass.all_reused(), "every memo reused: {:?}", tick.pass);
        assert!(
            tick.emission.written.is_empty() && tick.emission.kept == vec!["counter".to_string()],
            "digest-gated keep: {:?}",
            tick.emission
        );
        assert_eq!(sink.paths().len(), settled, "the sink is untouched");
    }

    #[test]
    fn tick_reemits_exactly_the_edited_unit() {
        // One edit: the sync reports it, the pass recomputes exactly that
        // unit, the emission rewrites exactly that stem.
        let mut s = Session::new();
        let mut sink = MemSink::new();
        s.tick(
            vec![(p("counter.tsx"), COUNTER_TSX)],
            &[],
            &mut sink,
            Profile::Layout,
            false,
        )
        .expect("the cold tick");
        let edited = COUNTER_TSX.replace("Add", "Add2");
        assert_ne!(edited, COUNTER_TSX);
        let tick = s
            .tick(
                vec![(p("counter.tsx"), edited.as_str())],
                &[],
                &mut sink,
                Profile::Layout,
                false,
            )
            .expect("the edit tick");
        assert_eq!(tick.sync.edited, vec![p("counter.tsx")]);
        assert_eq!(tick.pass.recompiled, vec![p("counter.tsx")]);
        assert_eq!(tick.emission.written, vec!["counter".to_string()]);
    }

    #[test]
    fn tick_refuses_a_denied_pass_fail_closed() {
        // BR-1 at the tick: ONE denied unit refuses the WHOLE emission —
        // nothing writes, not even the admitted unit's artifacts — and the
        // typed error names the blocking stem with the fix. The session
        // still carries the denial diagnostics (the sync and the compile
        // applied; only the emission refused).
        let mut s = Session::new();
        let mut sink = MemSink::new();
        let err = s
            .tick(
                vec![(p("counter.tsx"), COUNTER_TSX), (p("denied.tsx"), DENIED)],
                &[],
                &mut sink,
                Profile::Layout,
                false,
            )
            .expect_err("a denied pass refuses the tick");
        assert!(err.message.contains("denied"), "names the stem: {err}");
        assert!(err.message.contains("BR-1"), "names the contract: {err}");
        assert!(
            err.code().map(|c| c.to_string()).as_deref() == Some("ligature::emit"),
            "the stable code: {err:?}"
        );
        let help = err
            .help()
            .map(|h| h.to_string())
            .expect("miette help-text present");
        assert!(help.contains("explain"), "the help names the fix: {help}");
        assert!(
            sink.paths().is_empty(),
            "nothing wrote — not even the admitted unit: {:?}",
            sink.paths()
        );
        let unit = s.unit(&p("denied.tsx")).expect("the denied unit");
        assert!(
            unit.diagnostics.iter().any(|d| d.code == "LIG-E117"),
            "the denial rides the session: {:?}",
            unit.diagnostics
        );
    }

    #[test]
    fn tick_refuses_a_codegen_backstop_pass() {
        // The other BR-1 refusal: the codegen backstop failed one unit —
        // the pass is not fully admitted, the sink sees nothing.
        let mut s = Session::new();
        let mut sink = MemSink::new();
        let err = s
            .tick(
                vec![(p("state_fallback.tsx"), STATE_FALLBACK)],
                &[],
                &mut sink,
                Profile::Layout,
                false,
            )
            .expect_err("a codegen-failed pass refuses the tick");
        assert!(
            err.message.contains("codegen backstop"),
            "names the failure family: {err}"
        );
        assert!(
            err.message.contains("state_fallback"),
            "names the blocking stem: {err}"
        );
        assert!(sink.paths().is_empty(), "nothing wrote");
    }

    #[test]
    fn tick_prunes_units_that_left_the_session() {
        // Watch sync: a unit absent from the snapshot leaves the session
        // and its artifacts are pruned from the sink; the survivor's
        // digests still hold.
        let mut s = Session::new();
        let mut sink = MemSink::new();
        s.tick(
            vec![(p("a.tsx"), COUNTER_TSX), (p("b.tsx"), COUNTER_TSX)],
            &[],
            &mut sink,
            Profile::Layout,
            false,
        )
        .expect("two units emit");
        let tick = s
            .tick(
                vec![(p("a.tsx"), COUNTER_TSX)],
                &[],
                &mut sink,
                Profile::Layout,
                true,
            )
            .expect("the prune tick");
        assert_eq!(tick.sync.removed, vec![p("b.tsx")], "the snapshot drops b");
        assert!(tick.emission.written.is_empty(), "a's digest still holds");
        assert_eq!(tick.emission.pruned, vec!["b".to_string()]);
        assert!(
            sink.get("b.rs").is_none()
                && sink.get("ir/b.json").is_none()
                && sink.get("maps/b.map.json").is_none(),
            "b's artifact set is gone from the sink"
        );
        assert!(sink.get("a.rs").is_some(), "a's artifacts survive");
    }

    #[test]
    fn tick_on_an_emptied_session_emits_the_empty_manifest() {
        // The pinned divergence from `emit_to`: a snapshot that removes
        // EVERY source is a legitimate watch state — the tick writes the
        // empty manifest, records the empty build state, and prunes every
        // leftover artifact. (Where `emit_to` refuses an empty session,
        // there the empty set means "forgot to compile"; in a tick the
        // compile just ran, so empty means emptied.)
        let mut s = Session::new();
        let mut sink = MemSink::new();
        s.tick(
            vec![(p("a.tsx"), COUNTER_TSX), (p("b.tsx"), COUNTER_TSX)],
            &[],
            &mut sink,
            Profile::Layout,
            false,
        )
        .expect("two units emit");
        let tick = s
            .tick(
                Vec::<(PathBuf, &str)>::new(),
                &[],
                &mut sink,
                Profile::Layout,
                true,
            )
            .expect("the emptied-session tick prunes");
        assert_eq!(tick.sync.removed.len(), 2, "both sources left");
        assert!(tick.pass.all_reused(), "nothing left to compile");
        assert_eq!(tick.emission.pruned.len(), 2, "both stems pruned");
        let manifest = sink
            .get("manifest.toml")
            .and_then(|b| String::from_utf8(b.to_vec()).ok())
            .expect("the empty manifest wrote");
        assert!(manifest.contains("units = 0"), "zero units: {manifest}");
        assert!(
            sink.get("a.rs").is_none() && sink.get("b.rs").is_none(),
            "every artifact pruned"
        );
    }

    #[test]
    fn tick_with_reports_the_stage_events_one_to_one_with_the_spans() {
        // The observability convention (M19): the observed tick drives the
        // same stage events compile_with does — the cold pass reports
        // Computed per stage, the warm pass Reused at zero — plus the pass
        // report on on_pass.
        #[derive(Default)]
        struct Recording {
            events: Vec<(Stage, StageOutcome)>,
            passes: usize,
        }
        impl SessionObserver for Recording {
            fn on_stage(&mut self, event: &crate::observe::StageEvent) {
                self.events.push((event.stage, event.outcome));
            }
            fn on_pass(&mut self, _report: &crate::CompileReport) {
                self.passes += 1;
            }
        }
        let mut s = Session::new();
        let mut sink = MemSink::new();
        let mut rec = Recording::default();
        s.tick_with(
            vec![(p("counter.tsx"), COUNTER_TSX)],
            &[],
            &mut sink,
            Profile::Layout,
            false,
            &mut rec,
        )
        .expect("the cold tick");
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
        assert_eq!(rec.passes, 1, "one tick, one on_pass");
        rec.events.clear();
        s.tick_with(
            vec![(p("counter.tsx"), COUNTER_TSX)],
            &[],
            &mut sink,
            Profile::Layout,
            false,
            &mut rec,
        )
        .expect("the warm tick");
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
    }

    #[test]
    fn tick_matches_the_hand_assembled_watch_loop_byte_for_byte() {
        // The composition parity: the tick's artifact set is byte-identical
        // to the pre-seam watch loop's hand-assembled sync + compile +
        // emit_to over the same snapshot — the seam moves the composition
        // into the session, never the bytes.
        let mut looped_session = Session::new();
        looped_session.set_source(Path::new("counter.tsx"), COUNTER_TSX);
        looped_session.compile(Profile::Layout);
        let edited = COUNTER_TSX.replace("Add", "Add2");
        let mut loop_sink = MemSink::new();
        looped_session.sync(vec![(p("counter.tsx"), edited.as_str())], &[]);
        looped_session.compile(Profile::Layout);
        looped_session
            .emit_to(&mut loop_sink, Profile::Layout, false)
            .expect("the hand-assembled loop emits");

        let mut tick_session = Session::new();
        tick_session.set_source(Path::new("counter.tsx"), COUNTER_TSX);
        tick_session.compile(Profile::Layout);
        let mut tick_sink = MemSink::new();
        let tick: TickReport = tick_session
            .tick(
                vec![(p("counter.tsx"), edited.as_str())],
                &[],
                &mut tick_sink,
                Profile::Layout,
                false,
            )
            .expect("the tick emits");
        assert_eq!(
            tick.sync.edited,
            vec![p("counter.tsx")],
            "the tick saw the same edit"
        );
        for path in artifact_paths() {
            assert_eq!(
                tick_sink.get(&path),
                loop_sink.get(&path),
                "{path} is byte-identical across the two compositions"
            );
        }
    }

    #[test]
    fn tick_keeps_explicit_roots_like_the_cli_watch() {
        // The keep list rides through to the sync: a vanished explicit root
        // keeps its last registered text (the CLI watch passes its roots),
        // while a scanned source absent from the snapshot leaves.
        let mut s = Session::new();
        let mut sink = MemSink::new();
        s.tick(
            vec![(p("root.tsx"), COUNTER_TSX), (p("scan.tsx"), COUNTER_TSX)],
            &[],
            &mut sink,
            Profile::Layout,
            false,
        )
        .expect("two units emit");
        let tick = s
            .tick(
                vec![(p("root.tsx"), COUNTER_TSX)],
                &[p("root.tsx")],
                &mut sink,
                Profile::Layout,
                true,
            )
            .expect("the keep tick");
        let sync: &SyncReport = &tick.sync;
        assert_eq!(sync.removed, vec![p("scan.tsx")], "the absent scan leaves");
        assert!(
            s.sources().contains(&p("root.tsx")),
            "the kept root stays registered"
        );
    }
}
