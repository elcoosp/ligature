//! Runtime benchmarks (§14.2): `runtime::instantiate`, `runtime::update_mask`,
//! `runtime::list_diff_keyed`, `runtime::mount_lifecycle`,
//! `runtime::flush_checked_cycle`.

extern crate alloc;

use alloc::vec::Vec;
use criterion::{Criterion, criterion_group, criterion_main};
use lig_runtime::host::{
    AttrVal, EventKind, HandlerId, HookId, Host, NodeId, PropVal, StyleConstId, TemplateDesc,
};
use lig_runtime::mount::{ComponentUnit, Mounted};
use lig_runtime::{AnchorStep, DirtySet, Fragment, Reactions, Schedule, TplNode, plan_keyed};

struct CountingHost {
    commits: usize,
}

impl Host for CountingHost {
    fn create(&mut self, _node: NodeId, _desc: TemplateDesc<'_>) {
        self.commits += 1;
    }
    fn destroy(&mut self, _node: NodeId) {
        self.commits += 1;
    }
    fn set_text(&mut self, _node: NodeId, _text: &str) {
        self.commits += 1;
    }
    fn set_attr(&mut self, _node: NodeId, _key: &str, _val: Option<AttrVal<'_>>) {
        self.commits += 1;
    }
    fn set_class(&mut self, _node: NodeId, _class: Option<&str>) {
        self.commits += 1;
    }
    fn set_style(&mut self, _node: NodeId, _style: StyleConstId) {
        self.commits += 1;
    }
    fn set_style_dyn(&mut self, _node: NodeId, _props: &[(u32, PropVal<'_>)]) {
        self.commits += 1;
    }
    fn set_html(&mut self, _node: NodeId, _html: &str) {
        self.commits += 1;
    }
    fn insert(&mut self, _parent: NodeId, _child: NodeId, _index: u32) {
        self.commits += 1;
    }
    fn remove(&mut self, _parent: NodeId, _child: NodeId) {
        self.commits += 1;
    }
    fn move_(&mut self, _parent: NodeId, _child: NodeId, _index: u32) {
        self.commits += 1;
    }
    fn attach(&mut self, _node: NodeId, _ev: EventKind, _handler: HandlerId, _delegate: bool) {
        self.commits += 1;
    }
    fn detach(&mut self, _node: NodeId, _ev: EventKind, _handler: HandlerId) {
        self.commits += 1;
    }
    fn on_mount(&mut self, _node: NodeId, _hook: HookId) {
        self.commits += 1;
    }
    fn on_unmount(&mut self, _node: NodeId, _hook: HookId) {
        self.commits += 1;
    }
}

/// Counter-like template: div > (button, span+marker, branch marker).
fn counter_tpl() -> &'static [TplNode] {
    TPL
}

static TPL: &[TplNode] = &[TplNode::Element {
    tag: "div",
    attrs: &[],
    style: Some(StyleConstId(0)),
    children: &[
        TplNode::Element {
            tag: "button",
            attrs: &[],
            style: None,
            children: &[TplNode::Text { value: "Add" }],
        },
        TplNode::Element {
            tag: "span",
            attrs: &[],
            style: None,
            children: &[TplNode::Marker],
        },
        TplNode::Marker,
    ],
}];

static HOLE_ANCHORS: &[&[AnchorStep]] = &[
    &[AnchorStep::Down(0)],                            // button (event target)
    &[AnchorStep::Down(1), AnchorStep::MarkerDown(0)], // span text hole
];

static BRANCH_ANCHORS: &[&[AnchorStep]] = &[&[AnchorStep::Down(2)]];

struct NullReactions {
    dirty: DirtySet,
}

impl Reactions for NullReactions {
    fn schedule(&self) -> &'static Schedule {
        &Schedule {
            order: &[0, 1],
            writes: &[0, 0],
            writes_hi: &[],
        }
    }
    fn dirty(&mut self) -> &mut DirtySet {
        &mut self.dirty
    }
    fn run(&mut self, _h: &mut dyn Host, _r: u8) {}
}

/// The mount-seam workload's unit: the counter-shaped state one generated
/// `build` emits — a root fragment, a text bind reaction, and the
/// `ComponentUnit` impl the mount front drives.
struct BenchUnit {
    count: i64,
    frag: Fragment,
}

const BENCH_BIND: u8 = 0;

static BENCH_SCHEDULE: Schedule = Schedule {
    order: &[BENCH_BIND],
    writes: &[0],
    writes_hi: &[],
};

impl BenchUnit {
    fn build(h: &mut dyn Host) -> Self {
        let mut s = Self {
            count: 0,
            frag: Fragment::new(h, counter_tpl(), HOLE_ANCHORS, BRANCH_ANCHORS),
        };
        s.frag.mark_mask(1 << BENCH_BIND);
        lig_runtime::flush(&mut s, h);
        s
    }

    fn set_count(&mut self, v: i64) {
        self.count = v;
        self.frag.mark_mask(1 << BENCH_BIND);
    }
}

impl Reactions for BenchUnit {
    fn schedule(&self) -> &'static Schedule {
        &BENCH_SCHEDULE
    }
    fn dirty(&mut self) -> &mut DirtySet {
        &mut self.frag.dirty
    }
    fn run(&mut self, host: &mut dyn Host, r: u8) {
        if r == BENCH_BIND {
            let mut buf = [0u8; 20];
            let text = lig_runtime::format_i64(&mut buf, self.count);
            self.frag.set_text(host, 0, text);
        }
    }
}

impl ComponentUnit for BenchUnit {
    fn fragment(&mut self) -> &mut Fragment {
        &mut self.frag
    }
}

/// The checked-flush workload's schedule (M21): a ping-pong cycle — r0
/// writes r1, r1 writes r0 — the worst case the fail-safe cap bounds.
static CYCLE_SCHEDULE: Schedule = Schedule {
    order: &[0, 1],
    writes: &[1 << 1, 1 << 0],
    writes_hi: &[],
};

/// The ping-pong unit: never converges; `flush_checked` surfaces the
/// [`lig_runtime::CycleError`] after exactly `MAX_PASSES` passes.
struct CycleUnit {
    dirty: DirtySet,
}

impl Reactions for CycleUnit {
    fn schedule(&self) -> &'static Schedule {
        &CYCLE_SCHEDULE
    }
    fn dirty(&mut self) -> &mut DirtySet {
        &mut self.dirty
    }
    fn run(&mut self, _h: &mut dyn Host, _r: u8) {}
}

fn bench_runtime(c: &mut Criterion) {
    c.bench_function("runtime/instantiate", |b| {
        b.iter_batched(
            || CountingHost { commits: 0 },
            |mut h| Fragment::new(&mut h, counter_tpl(), HOLE_ANCHORS, BRANCH_ANCHORS),
            criterion::BatchSize::SmallInput,
        );
    });

    c.bench_function("runtime/update_mask", |b| {
        let mut host = CountingHost { commits: 0 };
        let mut frag = Fragment::new(&mut host, counter_tpl(), HOLE_ANCHORS, BRANCH_ANCHORS);
        let mut rx = NullReactions {
            dirty: DirtySet::new(),
        };
        b.iter(|| {
            frag.mark_mask((1 << 0) | (1 << 1));
            lig_runtime::flush(&mut rx, &mut host);
        });
    });

    c.bench_function("runtime/list_diff_keyed", |b| {
        // 64-item list, 8 shuffled — the mid-size reorder case, measured
        // through the shipped sequential planner (§7.5).
        let old: Vec<u64> = (0..64).map(|i| 100 + i).collect();
        let mut new = old.clone();
        new.swap(10, 40);
        new[60] = 999;
        b.iter(|| plan_keyed(&old, &new));
    });

    // The mount seam's full lifecycle (M20): mount, one save-tick through
    // `Mounted::update`, dispose — the three calls an embedding host loop
    // repeats per surface.
    c.bench_function("runtime/mount_lifecycle", |b| {
        b.iter_batched(
            || CountingHost { commits: 0 },
            |mut h| {
                let mut m = Mounted::mount(&mut h, BenchUnit::build);
                let tick = m.update(&mut h, |s| s.set_count(1));
                std::hint::black_box(tick.is_ok());
                m.dispose(&mut h);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // The checked flush's fail-safe cap (M21): the ping-pong cycle through
    // `flush_checked` — exactly MAX_PASSES capped passes, then the typed
    // error. Bounds the worst case the mount front can surface. The dirty
    // bit is boxed so the convergence loop cannot be const-folded away.
    c.bench_function("runtime/flush_checked_cycle", |b| {
        let mut host = CountingHost { commits: 0 };
        b.iter(|| {
            let mut c = CycleUnit {
                dirty: DirtySet::new(),
            };
            c.dirty.set(core::hint::black_box(0));
            let checked = lig_runtime::flush_checked(&mut c, &mut host);
            core::hint::black_box(checked.is_err())
        });
    });
}

criterion_group!(benches, bench_runtime);
criterion_main!(benches);
