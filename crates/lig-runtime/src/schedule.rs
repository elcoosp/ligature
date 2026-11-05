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
/// masks (`WRITES[r]` re-enters the flush as dirty bits, §7.2).
pub struct Schedule {
    /// Reaction indices in topological order (declaration-order ties).
    pub order: &'static [u8],
    /// Per reaction (indexed by reaction id): mask of reactions to re-dirty
    /// after it runs.
    pub writes: &'static [u64],
}

/// Pass cap before the runtime declares a reactive cycle (LIG-E401).
pub const MAX_PASSES: u8 = 32;

/// The §7.2 flush loop, verbatim semantics:
/// convergence when a full pass produces nothing new; `MAX_PASSES` cap is the
/// fail-safe behind the compile-time cycle Deny (R10).
///
/// # Panics
/// `LIG-E401 reactive cycle at runtime` when the schedule does not converge
/// within [`MAX_PASSES`] passes.
pub fn flush<R: Reactions>(rx: &mut R, host: &mut dyn Host) {
    let mut pending = rx.dirty().take();
    let mut pass = 0u8;
    while !pending.is_empty() {
        pass += 1;
        assert!(pass <= MAX_PASSES, "LIG-E401 reactive cycle at runtime");
        let mut produced = DirtySet::new();
        let schedule = rx.schedule();
        let low = pending.low();
        for &r in schedule.order {
            // pending bits are REACTION IDS (§7.2: `pending & bit(r)`).
            if low & (1u64 << r) != 0 {
                rx.run(host, r);
                if let Some(&w) = schedule.writes.get(r as usize) {
                    produced.set_mask(w);
                }
                produced.union(&rx.dirty().take());
            }
        }
        pending = produced;
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_PASSES, Reactions, Schedule, flush};
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
}
