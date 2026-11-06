//! The node slab (D-4, §10.1): `Fragment` owns its records; `NodeId` is a
//! `Copy` handle packing `(instance seq, local slab index)`. Generated code
//! never owns nodes. Disposal is per-fragment and total: teardown removes and
//! destroys in LIFO slab order.

use crate::dirty::DirtySet;
use crate::host::{AttrVal, EventKind, HandlerId, Host, NodeId, TemplateDesc};
use crate::template::{AnchorStep, Template, TplNode};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

static NEXT_INSTANCE: AtomicU32 = AtomicU32::new(0);

const SEQ_BITS: u32 = 16;
const IDX_MASK: u32 = (1 << SEQ_BITS) - 1;
const MAX_SLAB: usize = (1 << SEQ_BITS) - 1;

fn pack(seq: u32, idx: u32) -> NodeId {
    NodeId((seq << SEQ_BITS) | (idx & IDX_MASK))
}

fn unpack(id: NodeId) -> (u32, usize) {
    (id.0 >> SEQ_BITS, (id.0 & IDX_MASK) as usize)
}

/// The kind recorded on each slab entry (structure + destroy ordering).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// An element.
    Element,
    /// Static text.
    Text,
    /// A dynamic hole marker.
    Marker,
}

/// One slab record: structure links only (§10.1 — the host owns content).
#[derive(Debug, Clone, Copy)]
pub struct NodeRecord {
    /// Recorded kind.
    pub kind: NodeKind,
    /// Structure links.
    pub parent: Option<NodeId>,
    /// First child.
    pub first_child: Option<NodeId>,
    /// Next sibling.
    pub next_sibling: Option<NodeId>,
}

/// A mounted branch: its own sub-slab plus the parent marker it hangs on.
pub struct BranchInstance {
    /// The parent-side anchor node.
    pub anchor: NodeId,
    /// The branch's root node.
    pub root: NodeId,
    /// The branch's own fragment.
    pub frag: Box<Fragment>,
}

/// One mounted list item: its key, root node, and item fragment.
pub struct ListItem {
    /// The item key (`u64` — keys are interned/normalized at codegen).
    pub key: u64,
    /// The item's root node.
    pub root: NodeId,
    /// The item's own fragment (binds address relative anchors, §7.5).
    pub frag: Box<Fragment>,
}

/// A mounted list (§7.5 `For`): keyed items hanging on one anchor marker, in
/// visual order. Reconciliation is driven by `plan_keyed` (keyed lists,
/// since M5) / the index diff (unkeyed lists).
pub struct ListSlot {
    /// The parent-side anchor node.
    anchor: NodeId,
    /// Items in visual order.
    items: Vec<ListItem>,
}

/// A fragment instance: slab + holes + branch slots + handlers + dirty mask.
pub struct Fragment {
    ids: Vec<NodeId>,
    records: Vec<NodeRecord>,
    roots: Vec<NodeId>,
    holes: Vec<NodeId>,
    branch_holes: Vec<NodeId>,
    branches: Vec<Option<BranchInstance>>,
    lists: Vec<Option<ListSlot>>,
    slots: Vec<Option<BranchInstance>>,
    handlers: Vec<(NodeId, EventKind, HandlerId)>,
    /// Dirty reaction bits (the scheduler's storage, §7.2).
    pub dirty: DirtySet,
    seq: u32,
}

impl Fragment {
    /// Instantiate `tpl` on `host`; resolve hole anchors and branch anchors.
    ///
    /// `hole_anchors` address dynamic nodes (text/attr/event targets);
    /// `branch_anchors` address the marker nodes branch fragments mount on.
    pub fn new(
        host: &mut dyn Host,
        tpl: Template,
        hole_anchors: &[&[AnchorStep]],
        branch_anchors: &[&[AnchorStep]],
    ) -> Self {
        let seq = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
        let mut frag = Self {
            ids: Vec::new(),
            records: Vec::new(),
            roots: Vec::new(),
            holes: Vec::with_capacity(hole_anchors.len()),
            branch_holes: Vec::with_capacity(branch_anchors.len()),
            branches: Vec::new(),
            lists: Vec::new(),
            slots: Vec::new(),
            handlers: Vec::new(),
            dirty: DirtySet::new(),
            seq,
        };
        frag.instantiate(host, tpl);
        for a in hole_anchors {
            frag.holes.push(frag.resolve(a).unwrap_or(frag.ids[0]));
        }
        for a in branch_anchors {
            frag.branch_holes
                .push(frag.resolve(a).unwrap_or(frag.ids[0]));
        }
        frag
    }

    /// The instance sequence (slab identity).
    #[must_use]
    pub const fn seq(&self) -> u32 {
        self.seq
    }

    /// Number of live records.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether the slab is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// The template roots.
    #[must_use]
    pub fn roots(&self) -> &[NodeId] {
        &self.roots
    }

    /// Resolved hole addresses, in dynamic order.
    #[must_use]
    pub fn holes(&self) -> &[NodeId] {
        &self.holes
    }

    fn push(&mut self, kind: NodeKind) -> NodeId {
        let idx = self.records.len();
        assert!(
            idx < MAX_SLAB,
            "fragment slab overflow (65k nodes per instance)"
        );
        let id = pack(self.seq, idx as u32);
        self.ids.push(id);
        self.records.push(NodeRecord {
            kind,
            parent: None,
            first_child: None,
            next_sibling: None,
        });
        id
    }

    fn record(&self, id: NodeId) -> &NodeRecord {
        let (_, idx) = unpack(id);
        &self.records[idx]
    }

    fn record_mut(&mut self, id: NodeId) -> &mut NodeRecord {
        let (_, idx) = unpack(id);
        &mut self.records[idx]
    }

    fn instantiate(&mut self, host: &mut dyn Host, tpl: Template) {
        let mut prev: Option<NodeId> = None;
        for node in tpl {
            let id = self.build_node(host, node);
            if let Some(p) = prev {
                self.record_mut(p).next_sibling = Some(id);
            }
            prev = Some(id);
            self.roots.push(id);
        }
    }

    fn build_node(&mut self, host: &mut dyn Host, node: &TplNode) -> NodeId {
        match node {
            TplNode::Element {
                tag,
                attrs,
                style,
                children,
            } => {
                let id = self.push(NodeKind::Element);
                let attr_list: Vec<(&str, AttrVal<'_>)> =
                    attrs.iter().map(|a| (a.key, a.val)).collect();
                host.create(
                    id,
                    TemplateDesc {
                        tag,
                        attrs: &attr_list,
                        style: *style,
                    },
                );
                let mut prev: Option<NodeId> = None;
                for (position, child) in (*children).iter().enumerate() {
                    let child_id = self.build_node(host, child);
                    self.record_mut(child_id).parent = Some(id);
                    if let Some(p) = prev {
                        self.record_mut(p).next_sibling = Some(child_id);
                    } else {
                        self.record_mut(id).first_child = Some(child_id);
                    }
                    prev = Some(child_id);
                    host.insert(id, child_id, position as u32);
                }
                if let Some(style_id) = style {
                    host.set_style(id, *style_id);
                }
                id
            }
            TplNode::Text { value } => {
                let id = self.push(NodeKind::Text);
                host.create(
                    id,
                    TemplateDesc {
                        tag: "#text",
                        attrs: &[],
                        style: None,
                    },
                );
                host.set_text(id, value);
                id
            }
            TplNode::Marker => {
                let id = self.push(NodeKind::Marker);
                host.create(
                    id,
                    TemplateDesc {
                        tag: "#marker",
                        attrs: &[],
                        style: None,
                    },
                );
                id
            }
        }
    }

    /// Replay an anchor walk from the template root (2-instruction-per-step
    /// semantics over slab links, D-2).
    #[must_use]
    pub fn resolve(&self, steps: &[AnchorStep]) -> Option<NodeId> {
        let mut cur = *self.roots.first()?;
        for step in steps {
            cur = match step {
                AnchorStep::Down(i) => self.nth_child(cur, *i)?,
                AnchorStep::Next => self.record(cur).next_sibling?,
                AnchorStep::MarkerDown(i) => {
                    let n = self.nth_child(cur, *i)?;
                    if self.record(n).kind != NodeKind::Marker {
                        return None;
                    }
                    n
                }
            };
        }
        Some(cur)
    }

    fn nth_child(&self, node: NodeId, n: u32) -> Option<NodeId> {
        let mut cur = self.record(node).first_child?;
        for _ in 0..n {
            cur = self.record(cur).next_sibling?;
        }
        Some(cur)
    }

    /// A hole's node.
    #[must_use]
    pub fn hole(&self, index: usize) -> NodeId {
        self.holes.get(index).copied().unwrap_or(self.ids[0])
    }

    /// Apply a text update through a hole (the §9.2 `apply_text` shape).
    pub fn set_text(&self, host: &mut dyn Host, hole_index: usize, text: &str) {
        host.set_text(self.hole(hole_index), text);
    }

    /// Apply a class update through a hole.
    pub fn set_class(&self, host: &mut dyn Host, hole_index: usize, class: Option<&str>) {
        host.set_class(self.hole(hole_index), class);
    }

    /// Apply an attribute update through a hole.
    pub fn set_attr(
        &self,
        host: &mut dyn Host,
        hole_index: usize,
        key: &str,
        val: Option<AttrVal<'_>>,
    ) {
        host.set_attr(self.hole(hole_index), key, val);
    }

    /// Apply dynamic style props through a hole (prev-diffed by generated code).
    pub fn set_style_dyn(
        &self,
        host: &mut dyn Host,
        hole_index: usize,
        props: &[(u32, crate::host::PropVal<'_>)],
    ) {
        host.set_style_dyn(self.hole(hole_index), props);
    }

    /// Attach an event handler to a hole's node; records the slot.
    pub fn attach(
        &mut self,
        host: &mut dyn Host,
        hole_index: usize,
        ev: EventKind,
        handler: HandlerId,
        delegate: bool,
    ) {
        let node = self.hole(hole_index);
        self.handlers.push((node, ev, handler));
        host.attach(node, ev, handler, delegate);
    }

    /// Dispatch an event: find the handler slot and mark its reaction dirty
    /// (handler ids ARE reaction ids by generated convention, §10.1).
    pub fn dispatch(&mut self, node: NodeId, ev: EventKind) -> bool {
        let hit = self
            .handlers
            .iter()
            .find(|(n, e, _)| *n == node && *e == ev)
            .map(|(_, _, h)| *h);
        match hit {
            Some(h) => {
                self.dirty.set(h.0 as usize);
                true
            }
            None => false,
        }
    }

    /// Mark reactions dirty via a mask (the §9.2 setter pattern).
    pub fn mark_mask(&mut self, mask: u64) {
        self.dirty.set_mask(mask);
    }

    /// Mark all `reaction_count` reactions dirty (initial build pass).
    pub fn mark_all(&mut self, reaction_count: usize) {
        for r in 0..reaction_count {
            self.dirty.set(r);
        }
    }

    /// Mount a branch fragment on branch slot `index`.
    pub fn mount_branch(
        &mut self,
        host: &mut dyn Host,
        index: usize,
        tpl: Template,
        hole_anchors: &[&[AnchorStep]],
        branch_anchors: &[&[AnchorStep]],
    ) -> bool {
        if self.branches.get(index).is_some_and(Option::is_some) {
            return false; // already mounted (prev-gated by generated code)
        }
        let anchor = self
            .branch_holes
            .get(index)
            .copied()
            .unwrap_or_else(|| self.ids[0]);
        let sub = Fragment::new(host, tpl, hole_anchors, branch_anchors);
        let root = *sub.roots().first().unwrap_or(&self.ids[0]);
        host.insert(anchor, root, 0);
        while self.branches.len() <= index {
            self.branches.push(None);
        }
        self.branches[index] = Some(BranchInstance {
            anchor,
            root,
            frag: Box::new(sub),
        });
        true
    }

    /// Unmount (and totally dispose) a branch fragment.
    pub fn unmount_branch(&mut self, host: &mut dyn Host, index: usize) -> bool {
        if self.branches.get(index).is_none_or(Option::is_none) {
            return false;
        }
        let branch = self.branches[index].take().expect("checked");
        host.remove(branch.anchor, branch.root);
        branch.frag.teardown(host);
        true
    }

    /// Read-only access to a mounted branch's fragment.
    #[must_use]
    pub fn branch(&self, index: usize) -> Option<&Fragment> {
        self.branches
            .get(index)
            .and_then(|b| b.as_ref())
            .map(|b| &*b.frag)
    }

    /// Mutable access to a mounted branch's fragment.
    pub fn branch_mut(&mut self, index: usize) -> Option<&mut Fragment> {
        self.branches
            .get_mut(index)
            .and_then(|b| b.as_mut())
            .map(|b| &mut *b.frag)
    }

    /// Whether a branch slot is mounted.
    #[must_use]
    pub fn branch_is(&self, index: usize) -> bool {
        self.branches.get(index).is_some_and(Option::is_some)
    }

    /// Mount a list item at `index` with `key` on list slot `slot`
    /// (§7.5). The item fragment instantiates the item template; the host
    /// sees `insert(anchor, root, index)`.
    // (host, slot, index, key, tpl, holes, branches) — the template triple is
    // one logical unit (the mounted-fragment descriptor).
    #[allow(clippy::too_many_arguments)]
    pub fn list_insert(
        &mut self,
        host: &mut dyn Host,
        slot: usize,
        index: usize,
        key: u64,
        tpl: Template,
        hole_anchors: &[&[AnchorStep]],
        branch_anchors: &[&[AnchorStep]],
    ) -> bool {
        let anchor = self.ensure_list(slot);
        let sub = Fragment::new(host, tpl, hole_anchors, branch_anchors);
        let root = *sub.roots().first().unwrap_or(&self.ids[0]);
        host.insert(anchor, root, index as u32);
        let item = ListItem {
            key,
            root,
            frag: Box::new(sub),
        };
        let items = &mut self.lists[slot].as_mut().expect("list slot").items;
        let index = index.min(items.len());
        items.insert(index, item);
        true
    }

    /// Unmount (and totally dispose) the list item at `index`.
    pub fn list_remove(&mut self, host: &mut dyn Host, slot: usize, index: usize) -> bool {
        let Some(list) = self.lists.get_mut(slot).and_then(Option::as_mut) else {
            return false;
        };
        if index >= list.items.len() {
            return false;
        }
        let item = list.items.remove(index);
        host.remove(list.anchor, item.root);
        item.frag.teardown(host);
        true
    }

    /// Move a list item from `from` to `to` (both visual indices).
    pub fn list_move(&mut self, host: &mut dyn Host, slot: usize, from: usize, to: usize) {
        let Some(list) = self.lists.get_mut(slot).and_then(Option::as_mut) else {
            return;
        };
        if from >= list.items.len() || to >= list.items.len() {
            return;
        }
        let item = list.items.remove(from);
        list.items.insert(to, item);
        let root = list.items[to].root;
        let anchor = list.anchor;
        host.move_(anchor, root, to as u32);
    }

    /// Number of mounted items on a list slot.
    #[must_use]
    pub fn list_len(&self, slot: usize) -> usize {
        self.lists
            .get(slot)
            .and_then(Option::as_ref)
            .map_or(0, |l| l.items.len())
    }

    /// The key of the item at `index`.
    #[must_use]
    pub fn list_key(&self, slot: usize, index: usize) -> Option<u64> {
        self.lists
            .get(slot)
            .and_then(Option::as_ref)
            .and_then(|l| l.items.get(index))
            .map(|i| i.key)
    }

    /// Read-only access to an item fragment.
    #[must_use]
    pub fn list_item(&self, slot: usize, index: usize) -> Option<&Fragment> {
        self.lists
            .get(slot)
            .and_then(Option::as_ref)
            .and_then(|l| l.items.get(index))
            .map(|i| &*i.frag)
    }

    /// Mutable access to an item fragment (generated `apply_item` binds).
    pub fn list_item_mut(&mut self, slot: usize, index: usize) -> Option<&mut Fragment> {
        self.lists
            .get_mut(slot)
            .and_then(Option::as_mut)
            .and_then(|l| l.items.get_mut(index))
            .map(|i| &mut *i.frag)
    }

    /// Tear down every item of a list slot (the `For` toggled off).
    pub fn list_teardown(&mut self, host: &mut dyn Host, slot: usize) {
        let Some(list) = self.lists.get_mut(slot).and_then(Option::as_mut) else {
            return;
        };
        for item in list.items.drain(..).rev() {
            item.frag.teardown(host);
        }
    }

    /// Mount a slot fill (parent-supplied children, §7.4) at slot outlet
    /// `slot`. The fill is owned by this fragment and disposed with it; v1
    /// fills are static-by-construction (no dynamic binds inside).
    pub fn mount_slot(
        &mut self,
        host: &mut dyn Host,
        slot: usize,
        tpl: Template,
        hole_anchors: &[&[AnchorStep]],
        branch_anchors: &[&[AnchorStep]],
    ) -> bool {
        while self.slots.len() <= slot {
            self.slots.push(None);
        }
        if self.slots[slot].is_some() {
            return false;
        }
        let anchor = self.holes.get(slot).copied().unwrap_or_else(|| self.ids[0]);
        let sub = Fragment::new(host, tpl, hole_anchors, branch_anchors);
        let root = *sub.roots().first().unwrap_or(&self.ids[0]);
        host.insert(anchor, root, 0);
        self.slots[slot] = Some(BranchInstance {
            anchor,
            root,
            frag: Box::new(sub),
        });
        true
    }

    /// Whether a slot fill is mounted.
    #[must_use]
    pub fn slot_is(&self, slot: usize) -> bool {
        self.slots.get(slot).is_some_and(Option::is_some)
    }

    /// The anchor node of a branch/list slot (host tests, slot wiring).
    #[must_use]
    pub fn branch_anchor(&self, index: usize) -> Option<NodeId> {
        self.branch_holes.get(index).copied()
    }

    fn ensure_list(&mut self, slot: usize) -> NodeId {
        while self.lists.len() <= slot {
            self.lists.push(None);
        }
        if self.lists[slot].is_none() {
            let anchor = self
                .branch_holes
                .get(slot)
                .copied()
                .unwrap_or_else(|| self.ids[0]);
            self.lists[slot] = Some(ListSlot {
                anchor,
                items: Vec::new(),
            });
        }
        self.lists[slot].as_ref().expect("just set").anchor
    }

    /// Replace raw HTML at a hole (tiered dynamic, §6.2).
    pub fn set_html(&self, host: &mut dyn Host, hole_index: usize, html: &str) {
        host.set_html(self.hole(hole_index), html);
    }

    /// Total disposal in LIFO slab order (§10.1): own records, then mounted
    /// branches and list items (recursively, each in their own LIFO order).
    pub fn teardown(&self, host: &mut dyn Host) {
        for b in self.branches.iter().flatten() {
            b.frag.teardown(host);
        }
        for l in self.lists.iter().flatten() {
            for item in &l.items {
                item.frag.teardown(host);
            }
        }
        for s in self.slots.iter().flatten() {
            s.frag.teardown(host);
        }
        for id in self.ids.iter().rev() {
            host.destroy(*id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AnchorStep, Fragment, TplNode};
    use crate::host::{EvData, Host, NodeId, PropVal, TemplateDesc};
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;

    /// Recording host for fragment-structure tests (structure only).
    #[derive(Default)]
    struct RecHost {
        inserted: Vec<(NodeId, NodeId, u32)>,
        moved: Vec<(NodeId, NodeId, u32)>,
        removed: Vec<(NodeId, NodeId)>,
        destroyed: Vec<NodeId>,
        htmls: Vec<(NodeId, alloc::string::String)>,
        styles_dyn: Vec<usize>,
    }

    impl Host for RecHost {
        fn create(&mut self, _node: NodeId, _desc: TemplateDesc<'_>) {}
        fn destroy(&mut self, node: NodeId) {
            self.destroyed.push(node);
        }
        fn set_text(&mut self, _node: NodeId, _text: &str) {}
        fn set_attr(&mut self, _node: NodeId, _key: &str, _val: Option<crate::AttrVal<'_>>) {}
        fn set_class(&mut self, _node: NodeId, _class: Option<&str>) {}
        fn set_style(&mut self, _node: NodeId, _style: crate::StyleConstId) {}
        fn set_style_dyn(&mut self, _node: NodeId, _props: &[(u32, PropVal<'_>)]) {
            self.styles_dyn.push(1);
        }
        fn set_html(&mut self, node: NodeId, html: &str) {
            self.htmls.push((node, html.to_string()));
        }
        fn insert(&mut self, parent: NodeId, child: NodeId, index: u32) {
            self.inserted.push((parent, child, index));
        }
        fn remove(&mut self, parent: NodeId, child: NodeId) {
            self.removed.push((parent, child));
        }
        fn move_(&mut self, parent: NodeId, child: NodeId, index: u32) {
            self.moved.push((parent, child, index));
        }
        fn attach(
            &mut self,
            _node: NodeId,
            _ev: crate::EventKind,
            _handler: crate::HandlerId,
            _delegate: bool,
        ) {
        }
        fn detach(&mut self, _node: NodeId, _ev: crate::EventKind, _handler: crate::HandlerId) {}
        fn on_mount(&mut self, _node: NodeId, _hook: crate::HookId) {}
        fn on_unmount(&mut self, _node: NodeId, _hook: crate::HookId) {}
    }

    /// `<ul><!></ul>` — one list anchor under a `ul` root.
    fn list_tpl() -> crate::Template {
        &[TplNode::Element {
            tag: "ul",
            attrs: &[],
            style: None,
            children: &[TplNode::Marker],
        }]
    }

    /// `<li><!></li>` — an item with one text hole.
    fn item_tpl() -> crate::Template {
        &[TplNode::Element {
            tag: "li",
            attrs: &[],
            style: None,
            children: &[TplNode::Marker],
        }]
    }

    const ITEM_HOLES: &[&[AnchorStep]] = &[&[AnchorStep::Down(0), AnchorStep::MarkerDown(0)]];

    #[test]
    fn list_insert_tracks_keys_in_order() {
        let mut h = RecHost::default();
        let mut f = Fragment::new(&mut h, list_tpl(), &[], &[&[AnchorStep::MarkerDown(0)]]);
        f.list_insert(&mut h, 0, 0, 10, item_tpl(), ITEM_HOLES, &[]);
        f.list_insert(&mut h, 0, 1, 20, item_tpl(), ITEM_HOLES, &[]);
        f.list_insert(&mut h, 0, 1, 15, item_tpl(), ITEM_HOLES, &[]);
        assert_eq!(f.list_len(0), 3);
        assert_eq!(f.list_key(0, 0), Some(10));
        assert_eq!(f.list_key(0, 1), Some(15));
        assert_eq!(f.list_key(0, 2), Some(20));
        assert_eq!(f.list_key(0, 3), None);
        // one root insert per item onto the anchor (child inserts are intra-
        // template and have the item's li as parent).
        let anchor = f.branch_anchor(0).expect("anchor");
        assert_eq!(
            h.inserted.iter().filter(|(p, _, _)| *p == anchor).count(),
            3
        );
    }

    #[test]
    fn list_remove_disposes_item_lifo() {
        let mut h = RecHost::default();
        let mut f = Fragment::new(&mut h, list_tpl(), &[], &[&[AnchorStep::MarkerDown(0)]]);
        f.list_insert(&mut h, 0, 0, 1, item_tpl(), ITEM_HOLES, &[]);
        f.list_insert(&mut h, 0, 1, 2, item_tpl(), ITEM_HOLES, &[]);
        let anchor = f.branch_anchor(0).expect("anchor");
        let destroyed_before = h.destroyed.len();
        assert!(f.list_remove(&mut h, 0, 0));
        assert_eq!(f.list_len(0), 1);
        assert_eq!(f.list_key(0, 0), Some(2));
        // removal commit onto the anchor + LIFO teardown destroys
        assert_eq!(h.removed.iter().filter(|(p, _)| *p == anchor).count(), 1);
        assert!(h.destroyed.len() > destroyed_before);
        assert!(!f.list_remove(&mut h, 0, 5));
    }

    #[test]
    fn list_move_reorders_and_commits() {
        let mut h = RecHost::default();
        let mut f = Fragment::new(&mut h, list_tpl(), &[], &[&[AnchorStep::MarkerDown(0)]]);
        f.list_insert(&mut h, 0, 0, 1, item_tpl(), ITEM_HOLES, &[]);
        f.list_insert(&mut h, 0, 1, 2, item_tpl(), ITEM_HOLES, &[]);
        f.list_move(&mut h, 0, 1, 0);
        assert_eq!(f.list_key(0, 0), Some(2));
        assert_eq!(f.list_key(0, 1), Some(1));
        assert_eq!(h.moved.len(), 1);
    }

    #[test]
    fn list_teardown_drains_all_items() {
        let mut h = RecHost::default();
        let mut f = Fragment::new(&mut h, list_tpl(), &[], &[&[AnchorStep::MarkerDown(0)]]);
        f.list_insert(&mut h, 0, 0, 1, item_tpl(), ITEM_HOLES, &[]);
        f.list_insert(&mut h, 0, 1, 2, item_tpl(), ITEM_HOLES, &[]);
        f.list_teardown(&mut h, 0);
        assert_eq!(f.list_len(0), 0);
        // each item contributed one destroy (the li) + one text/marker child
        assert!(h.destroyed.len() >= 4, "{:?}", h.destroyed);
    }

    #[test]
    fn set_html_routes_through_the_commit_stream() {
        let mut h = RecHost::default();
        let f = Fragment::new(&mut h, list_tpl(), &[], &[]);
        f.set_html(&mut h, 0, "<b>x</b>");
        assert_eq!(h.htmls.len(), 1);
        assert_eq!(h.htmls[0].1, "<b>x</b>");
    }

    #[test]
    fn mount_slot_fills_once_and_disposes() {
        let mut h = RecHost::default();
        // Child template with one slot-outlet marker hole.
        let child_tpl: crate::Template = &[TplNode::Element {
            tag: "section",
            attrs: &[],
            style: None,
            children: &[TplNode::Marker],
        }];
        let mut f = Fragment::new(&mut h, child_tpl, &[&[AnchorStep::MarkerDown(0)]], &[]);
        assert!(f.mount_slot(&mut h, 0, item_tpl(), ITEM_HOLES, &[]));
        assert!(
            !f.mount_slot(&mut h, 0, item_tpl(), ITEM_HOLES, &[]),
            "once only"
        );
        assert!(f.slot_is(0));
        let before = h.destroyed.len();
        f.teardown(&mut h);
        assert!(h.destroyed.len() > before, "slot fill disposed with owner");
    }

    #[test]
    fn evdata_defaults_and_builders() {
        let ev = EvData::new();
        assert_eq!(ev.current_target_value, None);
        let ev = ev.current_target_value("3");
        assert_eq!(ev.current_target_value.as_deref(), Some("3"));
        let ev = ev.key("Enter");
        assert_eq!(ev.key.as_deref(), Some("Enter"));
    }

    #[test]
    fn branch_teardown_is_recursive() {
        // A branch holding a list must tear the list down too (§10.1 total
        // disposal).
        let mut h = RecHost::default();
        let mut f = Fragment::new(&mut h, list_tpl(), &[], &[&[AnchorStep::MarkerDown(0)]]);
        f.mount_branch(&mut h, 0, item_tpl(), ITEM_HOLES, &[]);
        let branch = f.branch_mut(0).expect("branch");
        branch.list_insert(&mut h, 0, 0, 7, item_tpl(), ITEM_HOLES, &[]);
        // NLL: the branch borrow ends here — teardown re-enters `f`.
        let before = h.destroyed.len();
        f.teardown(&mut h);
        assert!(h.destroyed.len() > before, "recursive teardown committed");
    }

    #[test]
    fn item_fragments_address_relative_anchors() {
        // §7.5: anchors are template-relative, so item moves never invalidate
        // addressing — each item's text hole is its own marker.
        let mut h = RecHost::default();
        let mut f = Fragment::new(&mut h, list_tpl(), &[], &[&[AnchorStep::MarkerDown(0)]]);
        f.list_insert(&mut h, 0, 0, 1, item_tpl(), ITEM_HOLES, &[]);
        f.list_insert(&mut h, 0, 1, 2, item_tpl(), ITEM_HOLES, &[]);
        let a = f.list_item(0, 0).expect("item 0").hole(0);
        let b = f.list_item(0, 1).expect("item 1").hole(0);
        assert_ne!(a, b);
        f.list_move(&mut h, 0, 1, 0);
        assert_eq!(f.list_item(0, 0).expect("moved").hole(0), b);
    }

    #[test]
    fn vec_of_steps_smoke() {
        let steps: Vec<AnchorStep> = vec![AnchorStep::Down(0), AnchorStep::Next];
        assert_eq!(steps.len(), 2);
        let boxed: alloc::boxed::Box<u8> = alloc::boxed::Box::new(1u8);
        assert_eq!(*boxed, 1);
    }
}
