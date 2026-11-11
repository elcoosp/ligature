//! The const-schedule flush loop (§7.2) — identical for every app: one
//! well-tested function, not per-app generated spaghetti.

use crate::dirty::DirtySet;
use crate::host::Host;

/// Reaction-execution surface implemented by generated component code.
pub trait Reactions {
    /// The component's const schedule (§7.2 `SCHEDULE`).
    fn schedule(&self) -> &'static Schedule;
    /// The dirty-bit storage (`frag.dirty` in generated code).
    fn dirty(&mut self) -> &mut DirtySet;
    /// Monomorphic dispatch — the patchable site (§9.2 `run`, §11.4).
    fn run(&mut self, host: &mut dyn Host, reaction: u8);
}

/// The const schedule: topological reaction order plus per-reaction write
/// masks (`WRITES[r]` re-enters the flush as dirty bits, §7.2). `writes_hi`
/// is the spill word for reactions 64..128 (per-bit dispatch refinement,
/// M3): parallel to `order`, indexed by reaction id minus the fast path.
pub struct Schedule {
    /// Reaction indices in topological order (declaration-order ties).
    pub order: &'static [u8],
    /// Per reaction (indexed by reaction id): mask of reactions to re-dirty
    /// after it runs (low word, ids 0..64).
    pub writes: &'static [u64],
    /// Per reaction (indexed by reaction id): spill mask of reactions to
    /// re-dirty after it runs (ids 64..128). Empty for components within the
    /// fast path.
    pub writes_hi: &'static [u64],
}

/// Pass cap before the runtime declares a reactive cycle (LIG-E401).
pub const MAX_PASSES: u8 = 32;

/// The reactive-cycle error (LIG-E401's typed seam, M21): the schedule
/// failed to converge within [`MAX_PASSES`] passes — reactions keep
/// re-dirtying each other, and the static cycle check should have denied
/// the shape at compile time. [`flush_checked`] reports it as data instead
/// of panicking; the catalog entry (`docs/errors/LIG-E401.md`) carries the
/// explanation and the fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CycleError {
    /// The pass cap the flush hit (the [`MAX_PASSES`] the loop enforced).
    pub passes: u8,
}

impl core::fmt::Display for CycleError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "LIG-E401 reactive cycle at runtime: the schedule did not converge \
             within {} passes",
            self.passes
        )
    }
}

impl core::error::Error for CycleError {}

/// The checked §7.2 flush loop (M21): the same convergence semantics as
/// [`flush`] — run due reactions in topological order, re-dirty through the
/// schedule's write masks, converge when a full pass produces nothing new —
/// reporting a non-converging schedule as [`CycleError`] instead of
/// panicking. The fail-closed form for embedders that cannot unwind
/// (`panic = "abort"` targets) or that want the failure as data; generated
/// code keeps calling [`flush`], which panics with the same catalog code.
pub fn flush_checked<R: Reactions + ?Sized>(
    rx: &mut R,
    host: &mut dyn Host,
) -> Result<(), CycleError> {
    let mut pending = rx.dirty().take();
    let mut pass = 0u8;
    while !pending.is_empty() {
        pass += 1;
        if pass > MAX_PASSES {
            return Err(CycleError { passes: MAX_PASSES });
        }
        let mut produced = DirtySet::new();
        let schedule = rx.schedule();
        let low = pending.low();
        for &r in schedule.order {
            // Per-bit dispatch: the fast path tests the pending low word
            // directly; spill reactions (id ≥ 64) read through the
            // `DirtySet` spill words (§7.2, M3 refinement).
            let due = if usize::from(r) < crate::dirty::REACTION_LIMIT {
                low & (1u64 << r) != 0
            } else {
                pending.get(usize::from(r))
            };
            if due {
                rx.run(host, r);
                if let Some(&w) = schedule.writes.get(r as usize) {
                    produced.set_mask(w);
                }
                if let Some(&w_hi) = schedule.writes_hi.get(r as usize) {
                    produced.mark_spill_word(0, w_hi);
                }
                produced.union(&rx.dirty().take());
            }
        }
        pending = produced;
    }
    Ok(())
}

/// The §7.2 flush loop, verbatim semantics:
/// convergence when a full pass produces nothing new; `MAX_PASSES` cap is the
/// fail-safe behind the compile-time cycle Deny (R10).
///
/// # Panics
/// `LIG-E401 reactive cycle at runtime` when the schedule does not converge
/// within [`MAX_PASSES`] passes — the panicking front over
/// [`flush_checked`], which reports the same failure as a typed
/// [`CycleError`].
pub fn flush<R: Reactions + ?Sized>(rx: &mut R, host: &mut dyn Host) {
    if let Err(cycle) = flush_checked(rx, host) {
        panic!("{cycle}");
    }
}

#[cfg(test)]
mod tests {
    use super::{CycleError, MAX_PASSES, Reactions, Schedule, flush, flush_checked};
    use crate::dirty::DirtySet;
    use crate::host::{Host, NodeId};

    struct NullHost;
    impl Host for NullHost {
        fn create(&mut self, _node: NodeId, _desc: crate::host::TemplateDesc<'_>) {}
        fn destroy(&mut self, _node: NodeId) {}
        fn set_text(&mut self, _node: NodeId, _text: &str) {}
        fn set_attr(&mut self, _node: NodeId, _key: &str, _val: Option<crate::host::AttrVal<'_>>) {}
        fn set_class(&mut self, _node: NodeId, _class: Option<&str>) {}
        fn set_style(&mut self, _node: NodeId, _style: crate::host::StyleConstId) {}
        fn set_style_dyn(&mut self, _node: NodeId, _props: &[(u32, crate::host::PropVal<'_>)]) {}
        fn set_html(&mut self, _node: NodeId, _html: &str) {}
        fn insert(&mut self, _parent: NodeId, _child: NodeId, _index: u32) {}
        fn remove(&mut self, _parent: NodeId, _child: NodeId) {}
        fn move_(&mut self, _parent: NodeId, _child: NodeId, _index: u32) {}
        fn attach(
            &mut self,
            _node: NodeId,
            _ev: crate::host::EventKind,
            _handler: crate::host::HandlerId,
            _delegate: bool,
        ) {
        }
        fn detach(
            &mut self,
            _node: NodeId,
            _ev: crate::host::EventKind,
            _handler: crate::host::HandlerId,
        ) {
        }
        fn on_mount(&mut self, _node: NodeId, _hook: crate::host::HookId) {}
        fn on_unmount(&mut self, _node: NodeId, _hook: crate::host::HookId) {}
    }

    /// 2-reaction chain: r1 (memo) writes r0's input, r0 reads it.
    struct Chain {
        dirty: DirtySet,
        ran: alloc::vec::Vec<u8>,
    }
    use alloc::string::String;

    impl Reactions for Chain {
        fn schedule(&self) -> &'static Schedule {
            &Schedule {
                order: &[1, 0],
                writes: &[0, 1 << 0],
                writes_hi: &[],
            }
        }
        fn dirty(&mut self) -> &mut DirtySet {
            &mut self.dirty
        }
        fn run(&mut self, _host: &mut dyn Host, r: u8) {
            self.ran.push(r);
        }
    }

    #[test]
    fn converges_after_writes_reenter() {
        let mut c = Chain {
            dirty: DirtySet::new(),
            ran: alloc::vec![],
        };
        c.dirty.set(1); // memo dirtied by a setter
        flush(&mut c, &mut NullHost);
        assert_eq!(
            c.ran,
            alloc::vec![1, 0],
            "memo runs first, its write re-dirties r0"
        );
        assert!(c.dirty.is_empty(), "converged: no pending bits");
    }

    #[test]
    fn cycle_panics_with_lig_e401() {
        struct Cyclic {
            dirty: DirtySet,
        }
        impl Reactions for Cyclic {
            fn schedule(&self) -> &'static Schedule {
                &Schedule {
                    order: &[0, 1],
                    writes: &[1 << 1, 1 << 0],
                    writes_hi: &[],
                }
            }
            fn dirty(&mut self) -> &mut DirtySet {
                &mut self.dirty
            }
            fn run(&mut self, _h: &mut dyn Host, _r: u8) {}
        }
        let mut c = Cyclic {
            dirty: DirtySet::new(),
        };
        c.dirty.set(0);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            flush(&mut c, &mut NullHost);
        }));
        let msg = result
            .err()
            .map(|e| {
                if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = e.downcast_ref::<&str>() {
                    alloc::string::String::from(*s)
                } else {
                    alloc::string::String::new()
                }
            })
            .unwrap_or_default();
        assert!(msg.contains("LIG-E401"), "{msg}");
    }

    #[test]
    fn max_passes_is_the_documented_cap() {
        assert_eq!(MAX_PASSES, 32);
    }

    /// A component whose reactions live in the spill word: r70 (id 70) is
    /// event-driven; a low memo r5 writes r70 through `writes_hi`.
    struct SpillDown {
        dirty: DirtySet,
        ran: alloc::vec::Vec<u8>,
    }

    impl Reactions for SpillDown {
        fn schedule(&self) -> &'static Schedule {
            &Schedule {
                order: &[5, 70],
                writes: &[0, 0],
                // indexed by reaction id: r5's spill write is bit (70-64).
                writes_hi: &[0, 0, 0, 0, 0, 1 << (70 - 64)],
            }
        }
        fn dirty(&mut self) -> &mut DirtySet {
            &mut self.dirty
        }
        fn run(&mut self, _host: &mut dyn Host, r: u8) {
            self.ran.push(r);
        }
    }

    #[test]
    fn spill_reactions_dispatch_per_bit() {
        let mut c = SpillDown {
            dirty: DirtySet::new(),
            ran: alloc::vec![],
        };
        c.dirty.set(5); // a low reaction with a spill write mask
        flush(&mut c, &mut NullHost);
        assert_eq!(
            c.ran,
            alloc::vec![5, 70],
            "the low reaction's writes_hi re-dirties reaction 70"
        );
        assert!(c.dirty.is_empty(), "converged");
    }

    #[test]
    fn spill_reactions_run_from_their_own_word() {
        let mut c = SpillDown {
            dirty: DirtySet::new(),
            ran: alloc::vec![],
        };
        c.dirty.set(70); // only the spill bit is set
        flush(&mut c, &mut NullHost);
        assert_eq!(c.ran, alloc::vec![70], "reaction 70 dispatches from spill");
    }

    /// A component whose reactions ping-pong forever: r0 writes r1, r1
    /// writes r0 — the schedule never converges (the fail-safe's workload).
    struct PingPong {
        dirty: DirtySet,
        ran: alloc::vec::Vec<u8>,
    }

    impl Reactions for PingPong {
        fn schedule(&self) -> &'static Schedule {
            &Schedule {
                order: &[0, 1],
                writes: &[1 << 1, 1 << 0],
                writes_hi: &[],
            }
        }
        fn dirty(&mut self) -> &mut DirtySet {
            &mut self.dirty
        }
        fn run(&mut self, _h: &mut dyn Host, r: u8) {
            self.ran.push(r);
        }
    }

    #[test]
    fn flush_checked_reports_the_cycle_instead_of_panicking() {
        let mut c = PingPong {
            dirty: DirtySet::new(),
            ran: alloc::vec![],
        };
        c.dirty.set(0);
        let err: CycleError =
            flush_checked(&mut c, &mut NullHost).expect_err("a cycle is an error");
        assert_eq!(err.passes, MAX_PASSES, "the error records the pass cap");
        let msg = alloc::format!("{err}");
        assert!(
            msg.contains("LIG-E401"),
            "the catalog code rides the error: {msg}"
        );
    }

    #[test]
    fn flush_checked_bounds_the_fail_safe_cost() {
        // The fail-safe caps the work: this ping-pong pair surfaces one due
        // reaction per pass (each pass's snapshot sees only the bit the
        // last reaction produced), so exactly MAX_PASSES reactions run
        // before the error surfaces — no runaway.
        let mut c = PingPong {
            dirty: DirtySet::new(),
            ran: alloc::vec![],
        };
        c.dirty.set(0);
        let _ = flush_checked(&mut c, &mut NullHost);
        assert_eq!(
            c.ran.len(),
            usize::from(MAX_PASSES),
            "one due reaction per capped pass, no runaway"
        );
    }

    #[test]
    fn flush_checked_converges_like_flush() {
        let mut c = Chain {
            dirty: DirtySet::new(),
            ran: alloc::vec![],
        };
        c.dirty.set(1);
        flush_checked(&mut c, &mut NullHost).expect("the chain converges");
        assert_eq!(c.ran, alloc::vec![1, 0], "identical schedule semantics");
    }
}
