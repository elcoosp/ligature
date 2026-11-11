//! The session-observer seam: structured, programmatic visibility into the
//! digest-gated pass. The M16 `tracing` spans NAME the stages; this seam
//! HANDS them to you — one [`StageEvent`] per stage per unit (`lower` →
//! `admit` → `codegen`), with the outcome and the stage's own wall clock,
//! plus the pass report on `on_pass` — no log subscriber required.
//!
//! 1. Compile two units through a timing observer: it totals per-stage wall
//!    clock across the pass (the replay a CI watcher or an IDE status bar
//!    would drive).
//! 2. Edit ONE source and compile again: the observer sees exactly that
//!    unit's stages recompute — the other unit's stages report `Reused` at
//!    zero elapsed (the D-9 digest gate, as events).
//! 3. Print the stage table and the pass reports.
//!
//! Run: `cargo run -p lig-incremental --example observed_pass`

use lig_admission::Profile;
use lig_incremental::Session;
use lig_incremental::observe::{SessionObserver, Stage, StageEvent, StageOutcome};
use std::collections::BTreeMap;
use std::time::Duration;

/// A timing observer: totals per-stage wall clock and counts outcomes per
/// stage — the shape a build dashboard wants (caller-owned state, readable
/// the moment the pass returns).
#[derive(Default)]
struct TimingObserver {
    totals: BTreeMap<Stage, Duration>,
    outcomes: BTreeMap<Stage, [usize; 3]>, // [computed, failed, reused]
    passes: usize,
}

impl SessionObserver for TimingObserver {
    fn on_stage(&mut self, event: &StageEvent) {
        *self.totals.entry(event.stage).or_default() += event.elapsed;
        let slot = match event.outcome {
            StageOutcome::Computed => 0,
            StageOutcome::Failed => 1,
            StageOutcome::Reused => 2,
        };
        self.outcomes.entry(event.stage).or_default()[slot] += 1;
    }

    fn on_pass(&mut self, report: &lig_incremental::CompileReport) {
        self.passes += 1;
        println!(
            "pass: revision {} — {} recompiled, {} reused in {:.2} ms",
            report.revision,
            report.recompiled.len(),
            report.reused.len(),
            report.elapsed.as_secs_f64() * 1000.0
        );
    }
}

fn main() {
    // 1. One cold pass over two units, observed: every stage recomputes
    //    and reports.
    let source = include_str!("../../../fixtures/conformance/counter/input.solid.tsx");
    let mut session = Session::new();
    session.set_source("counter.tsx", source);
    session.set_source("hero.tsx", source);
    let mut observer = TimingObserver::default();
    session.compile_with(Profile::Layout, &mut observer);

    // 2. An edit recompiles exactly the edited unit's stages; the other
    //    unit's stages report `Reused` at zero elapsed (the digest gate,
    //    as events).
    let edited = source.replace("Add", "Add2");
    assert_ne!(edited, source, "the edit is real");
    session.set_source("counter.tsx", edited);
    session.compile_with(Profile::Layout, &mut observer);

    // 3. The stage table, as the observer saw it.
    println!("\nstage        total      computed  failed  reused");
    for stage in [Stage::Lower, Stage::Admit, Stage::Codegen] {
        let total = observer.totals.get(&stage).copied().unwrap_or_default();
        let [c, f, r] = observer.outcomes.get(&stage).copied().unwrap_or_default();
        println!(
            "{:<12} {:>7.2} ms {:>8} {:>7} {:>7}",
            stage.span_name(),
            total.as_secs_f64() * 1000.0,
            c,
            f,
            r
        );
    }
    println!("\nthe same stages the M16 tracing spans name — now as events a consumer owns");
}
