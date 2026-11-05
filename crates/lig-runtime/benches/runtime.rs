//! Runtime benchmarks (§14.2): `runtime::instantiate`, `runtime::update_mask`,
//! `runtime::list_diff_keyed`.

extern crate alloc;

use alloc::vec::Vec;
use criterion::{Criterion, criterion_group, criterion_main};
use lig_runtime::host::{
    AttrVal, EventKind, HandlerId, HookId, Host, NodeId, PropVal, StyleConstId, TemplateDesc,
};
use lig_runtime::{AnchorStep, DirtySet, Fragment, Reactions, Schedule, TplNode, keyed_diff};

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
        }
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
        // 64-item list, 8 shuffled — the mid-size reorder case
        let old: Vec<u64> = (0..64).map(|i| 100 + i).collect();
        let mut new = old.clone();
        new.swap(10, 40);
        new[60] = 999;
        b.iter(|| keyed_diff(&old, &new));
    });
}

criterion_group!(benches, bench_runtime);
criterion_main!(benches);
