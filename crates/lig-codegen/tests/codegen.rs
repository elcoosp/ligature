//! Codegen contract tests: byte-reproducibility (D-8), shape checks against
//! §9.2, and the fail-closed subset boundaries.

use lig_codegen::generate_component;
use lig_parse_solid::tests_support::COUNTER_TSX;

fn counter_code() -> String {
    let module = lig_parse_solid::lower_module(COUNTER_TSX, "counter.tsx").expect("lower");
    let unit = generate_component(&module, lig_core::id::ComponentId::new(0)).expect("codegen");
    unit.text
}

#[test]
fn counter_generates_with_expected_shape() {
    let text = counter_code();
    assert!(text.contains("pub struct Counter {"), "{text}");
    assert!(text.contains("pub count: i64"));
    assert!(text.contains("pub doubled: i64"));
    assert!(text.contains("pub fn build(h: &mut dyn Host, props: CounterProps) -> Self"));
    assert!(text.contains("impl ::lig_runtime::Reactions for Counter"));
    assert!(text.contains("pub fn set_count"));
    assert!(text.contains("TPL_T"));
    assert!(text.contains("AnchorStep::Down"));
    // the memo gate (§7.2 equality-gated memos)
    assert!(text.contains("if next != self.doubled"));
}

#[test]
fn counter_codegen_is_byte_reproducible() {
    let a = counter_code();
    let b = counter_code();
    assert_eq!(a, b, "two generations must be byte-identical (D-8)");
}

#[test]
fn event_glue_dispatches_through_the_flush_loop() {
    let text = counter_code();
    assert!(text.contains("pub fn fire_"), "{text}");
    assert!(text.contains("ev: &::lig_runtime::EvData"), "{text}");
    assert!(text.contains("::lig_runtime::flush(self, h)"));
    assert!(text.contains("EventKind::Click"), "{text}");
}

#[test]
fn schedule_is_a_program_constant() {
    let text = counter_code();
    assert!(
        text.contains("static Counter_SCHEDULE: ::lig_runtime::Schedule"),
        "{text}"
    );
    assert!(text.contains("order: &["));
    assert!(text.contains("writes: &["));
}

#[test]
fn props_generate_a_props_struct_and_setter() {
    let src = r"export function Greet({ name }: { name: string }) {
  return <p>{name}</p>;
}
";
    let module = lig_parse_solid::lower_module(src, "greet.tsx").expect("lower");
    let unit = generate_component(&module, lig_core::id::ComponentId::new(0)).expect("props (M1)");
    // §7.6: props are plain struct fields; the parent passes a <Name>Props.
    assert!(unit.text.contains("pub struct GreetProps"), "{}", unit.text);
    assert!(unit.text.contains("pub name: ::alloc::string::String"));
    assert!(unit.text.contains("props.name"));
    // §7.6: setters are the only write path.
    assert!(unit.text.contains("pub fn set_name"));
}

#[test]
fn snapshot_counter_generated_code() {
    let text = counter_code();
    insta::assert_snapshot!("counter_generated", text);
}
#[test]
fn spill_path_emits_exact_per_bit_masks_and_writes_hi() {
    // A synthetic 70-reaction component: one signal read by 70 effects. The
    // init masks must be exact per bit (low word full, spill word 0b111111),
    // and the schedule carries a (zeroed) writes_hi row — the M3 dispatch
    // refinement over the former mark-everything spill path.
    use lig_ir::expr::{Expr, Lit};
    use lig_ir::fragment::{ComponentDecl, FragmentIr, StateDecl, StateKind};
    use lig_ir::module::{ReactionDecl, ReactionKind, Tier};

    let mut m = lig_ir::ViewModule::new(lig_ir::Frontend::Solid);
    let tpl = m.intern_template(lig_ir::template::TemplateIr::new(
        lig_ir::template::NodeIr::Marker,
    ));
    let name = m.strings.intern("Big");
    let root = m.push_fragment(FragmentIr {
        name,
        template: tpl,
        dynamics: vec![],
        children: vec![],
        props: vec![],
        returns: vec![],
    });
    m.components.push(ComponentDecl {
        name,
        root,
        props: vec![],
    });
    let count = m.strings.intern("count");
    let init = m.insert_expr(Expr::Lit(Lit::Int(0)));
    m.states.push(StateDecl {
        name: count,
        owner: root,
        kind: StateKind::Signal,
        ty: lig_ir::Ty::Int,
        init: Some(init),
    });
    for i in 0..70 {
        let body = m.insert_expr(Expr::Lit(Lit::Int(i)));
        m.reactions.push(ReactionDecl {
            kind: ReactionKind::Effect { tier: Tier::Post },
            reads: vec![count],
            writes: vec![],
            body,
            owner: root,
        });
    }
    let unit = generate_component(&m, lig_core::id::ComponentId::new(0)).expect("codegen");
    let text = unit.text;
    assert!(
        text.contains("const Big_N_REACTIONS: usize = 70usize;"),
        "{text}"
    );
    assert!(
        text.contains("writes_hi: &["),
        "schedule carries the spill row: {text}"
    );
    assert!(
        text.contains("mark_mask(0xffffffffffffffff)"),
        "low init word is exact: {text}"
    );
    assert!(
        text.contains("mark_spill_word(0, 0x3f)"),
        "spill init word is exact (70 − 64 bits): {text}"
    );
    assert!(text.contains("const Big_R69_FX: u8 = 69u8;"), "{text}");
}
#[test]
fn ref_captures_split_by_addressing_context() {
    // M7: a ref captures the hole NodeId of the fragment it lives on. The
    // four addressing contexts emit four capture phases: direct refs in the
    // build body, fill refs through the mounted content, fallback refs
    // through the child's outlet (the parent writes the child's `pub`
    // field), branch refs after the initial flush mounts the branch.
    let src = r#"export function Panel({ title }) {
  const [open, setOpen] = createSignal(true);
  let foot;
  let branched;
  return (
    <section>
      <Slot name="header" />
      <Slot name="foot">
        <i ref={foot}>{title}</i>
      </Slot>
      <Show when={open()}>
        <em ref={branched}>{title}</em>
      </Show>
    </section>
  );
}

export function App() {
  const [label, setLabel] = createSignal("live");
  let direct;
  let fillEl;
  return (
    <div ref={direct}>
      <Panel title="Status" header={<b ref={fillEl}>{label}</b>} />
    </div>
  );
}
"#;
    let module = lig_parse_solid::lower_module(src, "refs.tsx").expect("lower");
    let unit = lig_codegen::generate_module(&module).expect("codegen");
    let text = unit.text;
    // Direct ref: the build body captures from `self.frag`.
    assert!(
        text.contains("s.ref_direct = s.frag.hole("),
        "direct ref captures in the build body: {text}"
    );
    // Fill ref: captured through the mounted content, into the parent's field.
    assert!(
        text.contains("if let Some(c) = s.panel.frag.slot_fill(0usize)"),
        "fill ref capture guards on the mounted content: {text}"
    );
    assert!(
        text.contains("s.ref_fillEl = c.hole("),
        "fill ref writes the parent's field: {text}"
    );
    // Fallback ref: the parent writes the CHILD's pub field at mount time.
    assert!(
        text.contains("if let Some(c) = s.panel.frag.slot_fallback(1usize)"),
        "fallback ref capture guards on the mounted outlet: {text}"
    );
    assert!(
        text.contains("s.panel.ref_foot = c.hole("),
        "fallback ref writes the child's field: {text}"
    );
    // Branch ref: captured after the initial flush mounts the branch.
    assert!(
        text.contains("if let Some(b) = s.frag.branch(0usize)"),
        "branch ref capture guards on the mounted branch: {text}"
    );
    assert!(
        text.contains("s.ref_branched = b.hole("),
        "branch ref writes the component's field: {text}"
    );
    // Every ref field starts as the absent sentinel.
    assert!(
        text.contains("ref_direct: ::lig_runtime::NodeId(u32::MAX)"),
        "sentinel init: {text}"
    );
}

#[test]
fn composed_contexts_address_content_branches_and_lists() {
    // M8: structural slot content composes the addressing walk. An If on a
    // fill mounts its branch ON the content (`slot_fill_mut` → the content
    // receiver), the branch's own binds walk through the composed chain
    // (`slot_fill_mut` → `branch_mut`), a For on the fill reconciles the
    // content's own list table through the associated receiver-param
    // reconcile, and the mount marks the fresh branch's binds (the flush
    // re-enters and initializes them).
    let src = r#"export function App() {
  const [on, setOn] = createSignal(true);
  const [label, setLabel] = createSignal("live");
  const [rows, setRows] = createSignal(["alpha"]);
  return (
    <Panel
      title="Status"
      header={
        <div>
          <Show when={on()}>
            <b>{label}</b>
          </Show>
          <For each={rows()} key={(t) => t}>{(t) => <li>{t}</li>}</For>
        </div>
      }
    />
  );
}

export function Panel({ title }) {
  return <section><Slot name="header" /></section>;
}
"#;
    let module = lig_parse_solid::lower_module(src, "structural.tsx").expect("lower");
    let text = lig_codegen::generate_module(&module).expect("codegen").text;
    // The If mount rides the content receiver, then flags the mount.
    assert!(
        text.contains("if let Some(c) = self.panel.frag.slot_fill_mut(")
            && text.contains("c.mount_branch("),
        "content If mounts through the fill content: {text}"
    );
    assert!(
        text.contains("let mut did_mount = false;") && text.contains("did_mount = true;"),
        "the mount is flagged for the init marking: {text}"
    );
    // The fresh branch's binds are marked on the component's dirty set —
    // the flush re-enters and initializes them.
    assert!(
        text.contains("if did_mount {") && text.contains("self.frag.mark_mask(0x"),
        "mount-time marking initializes the fresh branch's binds: {text}"
    );
    // The branch's own bind walks the composed chain.
    assert!(
        text.contains("if let Some(c) = self.panel.frag.slot_fill_mut(")
            && text.contains("if let Some(b) = c.branch_mut(")
            && text.contains("b.set_text(h,"),
        "branch bind addresses content → branch: {text}"
    );
    // The For reconciles the content's list table through the receiver
    // param (the associated fn takes no self).
    assert!(
        text.contains("Self::reconcile_f"),
        "composed reconcile is a receiver-param associated fn: {text}"
    );
    assert!(
        text.contains("recv: &mut ::lig_runtime::Fragment"),
        "the composed reconcile addresses its own receiver fragment: {text}"
    );
}

#[test]
fn branch_nested_branches_compose_the_walk() {
    // M8: nested Ifs compose (Branch { base: Branch { base: Direct } }) —
    // the inner bind walks branch → branch, not self.frag → branch.
    let src = r#"export function App() {
  const [a, setA] = createSignal(true);
  const [b, setB] = createSignal(true);
  const [label, setLabel] = createSignal("in");
  return (
    <div>
      <Show when={a()}>
        <span>
          <Show when={b()}>
            <em>{label}</em>
          </Show>
        </span>
      </Show>
    </div>
  );
}
"#;
    let module = lig_parse_solid::lower_module(src, "nested.tsx").expect("lower");
    let text = lig_codegen::generate_module(&module).expect("codegen").text;
    // The inner bind unwraps the OUTER branch first, then the inner one.
    assert!(
        text.contains("if let Some(b) = self.frag.branch_mut(")
            && text.contains("if let Some(b) = b.branch_mut("),
        "nested branch binds walk branch → branch: {text}"
    );
}

#[test]
fn content_components_insert_at_mount_and_pass_props() {
    // M9 grandchild prop-pass: a component use inside slot content keeps the
    // Direct ownership shape (the grandchild is the content author's struct
    // field), and gains a MOUNT-TIME story — the parent's slot assembly
    // inserts the grandchild's DOM root into the content's component hole
    // through the composed read walk, and the prop-pass run arm (the
    // content author's bind reaction) sets the props and flushes the
    // grandchild instance.
    let src = r#"export function App() {
  const [n, setN] = createSignal(2);
  return (
    <Panel title="x" header={<Badge count={n()} />} />
  );
}

export function Badge({ count }) {
  return <b class="badge">{count}</b>;
}

export function Panel({ title }) {
  return <section><Slot name="header" /></section>;
}
"#;
    let module = lig_parse_solid::lower_module(src, "grandchild.tsx").expect("lower");
    let text = lig_codegen::generate_module(&module).expect("codegen").text;
    // The grandchild is the content author's struct field.
    assert!(
        text.contains("pub badge: Badge"),
        "grandchild instance is the author's field: {text}"
    );
    // The insert rides the mounted content's component hole.
    assert!(
        text.contains("if let Some(c) = s.panel.frag.slot_fill(")
            && text.contains("h.insert(c.hole(")
            && text.contains("s.badge.frag.roots()[0]"),
        "the grandchild root inserts into the mounted content: {text}"
    );
    // The prop-pass arm writes the grandchild's setters and flushes it.
    assert!(
        text.contains("self.badge.set_count(")
            && text.contains("::lig_runtime::flush(&mut self.badge, h)"),
        "the prop-pass arm addresses the grandchild instance: {text}"
    );
}

#[test]
fn nested_outlets_mount_through_the_composed_walk() {
    // M9 composed fill contexts: a nested outlet on fill content mounts its
    // use-site fill (or its own fallback) at content-mount time through the
    // composed accessor chain, and a bind on the nested fill walks the
    // two-level `slot_fill_mut` unwrap.
    let src = r#"export function App() {
  const [tag, setTag] = createSignal("in");
  return (
    <Panel
      title="x"
      header={<div><Slot name="inner"><em>none</em></Slot></div>}
      inner={<b>{tag}</b>}
    />
  );
}

export function Panel({ title }) {
  return <section><Slot name="header" /></section>;
}
"#;
    let module = lig_parse_solid::lower_module(src, "nested.tsx").expect("lower");
    let text = lig_codegen::generate_module(&module).expect("codegen").text;
    // The nested fill mounts ON the mounted header fill (build body).
    assert!(
        text.contains("if let Some(c) = s.panel.frag.slot_fill_mut(0usize)")
            && text.contains("c.mount_slot("),
        "the nested outlet's fill mounts through the content walk: {text}"
    );
    // A bind on the nested fill composes the walk: content → nested fill.
    assert!(
        text.contains("if let Some(c) = self.panel.frag.slot_fill_mut(")
            && text.contains("if let Some(c) = c.slot_fill_mut(")
            && text.contains("c.set_text(h,"),
        "the nested fill bind walks content → nested fill: {text}"
    );
}

#[test]
fn unfilled_nested_outlets_mount_their_own_fallback() {
    // M9: a nested outlet the use site does not fill mounts the outlet's
    // OWN fallback (author-side statics), still through the composed walk.
    let src = r#"export function App() {
  return <Panel title="x" header={<div><Slot name="inner"><em>none</em></Slot></div>} />;
}

export function Panel({ title }) {
  return <section><Slot name="header" /></section>;
}
"#;
    let module = lig_parse_solid::lower_module(src, "nested_fb.tsx").expect("lower");
    let text = lig_codegen::generate_module(&module).expect("codegen").text;
    assert!(
        text.contains("if let Some(c) = s.panel.frag.slot_fill_mut(0usize)")
            && text.contains("::lig_runtime::SlotOrigin::Fallback"),
        "the unfilled nested outlet mounts its own fallback: {text}"
    );
}

#[test]
fn fills_matching_no_outlet_fail_closed() {
    // M9 backstop: a fill whose name matches no outlet on the child's root
    // or fallback contents is a compile error — the M6-era silent drop
    // (the fill never mounted, nobody noticed) closes.
    let src = r#"export function App() {
  return <Panel title="x" nowhere={<b>lost</b>} />;
}

export function Panel({ title }) {
  return <section><Slot name="foot" /></section>;
}
"#;
    let module = lig_parse_solid::lower_module(src, "lost.tsx").expect("lower");
    let err = lig_codegen::generate_module(&module).expect_err("unmatched fill denies");
    assert!(
        matches!(err, lig_codegen::CodegenError::Unsupported(_)),
        "unmatched fill is Unsupported: {err:?}"
    );
}

#[test]
fn direct_component_uses_attach_their_root_at_the_use_hole() {
    // M10: the M2-era silent drop — a direct child's root never entered the
    // parent's tree (the commit stream had no `Host::insert` at the use
    // hole; the child's DOM floated detached). The build body now attaches
    // every direct use's root before slot assembly, so the initial flush
    // writes props onto a tree-attached child.
    let src = r#"export function App() {
  return <Panel title="Status"><p>all green</p></Panel>;
}

export function Panel({ title, children }) {
  return <section><h1>{title}</h1><Slot /></section>;
}
"#;
    let module = lig_parse_solid::lower_module(src, "attach.tsx").expect("lower");
    let text = lig_codegen::generate_module(&module).expect("codegen").text;
    assert!(
        text.contains("h.insert(s.frag.hole(0usize), s.panel.frag.roots()[0], 0);"),
        "the child root attaches at the use hole in the build body: {text}"
    );
}

#[test]
fn branch_components_rebuild_per_mount() {
    // M10 per-mount instance identity: a component use inside a branch body
    // (content-rooted here) builds as an Option field; the mounting If arm
    // rebuilds the instance and inserts its root into the branch's component
    // hole; the unmount arm disposes the instance; the prop-pass arm guards
    // on the presence of the instance.
    let src = r#"export function App() {
  const [on, setOn] = createSignal(true);
  const [n, setN] = createSignal("two");
  return (
    <Panel title="x" header={<Show when={on()}><Badge count={n()} /></Show>} />
  );
}

export function Badge({ count }) {
  return <b class="badge">{count}</b>;
}

export function Panel({ title }) {
  return <section><Slot name="header" /></section>;
}
"#;
    let module = lig_parse_solid::lower_module(src, "permount.tsx").expect("lower");
    let text = lig_codegen::generate_module(&module).expect("codegen").text;
    // The instance field is per-mount (Option), built at mount time.
    assert!(
        text.contains("pub badge: Option<Badge>"),
        "branch components are Option fields: {text}"
    );
    assert!(
        text.contains("self.badge = Some(Badge::build(h, BadgeProps::default()));"),
        "the mounting arm rebuilds the instance: {text}"
    );
    assert!(
        text.contains("self.badge.take()"),
        "the unmount arm resets the instance: {text}"
    );
    assert!(
        text.contains("inst.frag.teardown(h);"),
        "the unmount arm disposes the instance's slab: {text}"
    );
    // The grandchild root inserts into the branch's component hole.
    assert!(
        text.contains("h.insert(n, inst.frag.roots()[0], 0);"),
        "the fresh root inserts into the branch hole: {text}"
    );
    // The prop-pass arm guards on the per-mount instance.
    assert!(
        text.contains("if let Some(c) = self.badge.as_mut()"),
        "the prop-pass arm guards on the instance: {text}"
    );
}

#[test]
fn branch_outlets_mount_fills_and_fallbacks_with_the_branch() {
    // M10 branch-time slot assembly: an outlet inside a branch body of slot
    // content mounts its use-site fill (or the outlet's own fallback) when
    // the branch mounts — through the composed walk (branch → slot_fill_mut).
    let src = r#"export function App() {
  const [on, setOn] = createSignal(true);
  const [tag, setTag] = createSignal("in");
  return (
    <Panel
      title="x"
      header={<Show when={on()}><Slot name="inner"><em>none</em></Slot></Show>}
      inner={<b class="hl">{tag}</b>}
    />
  );
}

export function Panel({ title }) {
  return <section><Slot name="header" /></section>;
}
"#;
    let module = lig_parse_solid::lower_module(src, "branchslot.tsx").expect("lower");
    let text = lig_codegen::generate_module(&module).expect("codegen").text;
    // The branch-internal outlet's fill mounts through the composed walk.
    assert!(
        text.contains(".branch(0usize)") || text.contains("branch_mut(0usize)"),
        "the walk unwraps the branch: {text}"
    );
    assert!(
        text.contains("b.mount_slot(") && text.contains("::lig_runtime::SlotOrigin::Fill"),
        "the fill mounts at branch-mount time: {text}"
    );
    // The bind on the filled content walks branch → fill.
    assert!(
        text.contains("if let Some(b) =") && text.contains("if let Some(c) = b.slot_fill_mut("),
        "the fill bind walks the composed chain: {text}"
    );
}

#[test]
fn own_branch_outlets_mount_their_fallback_at_branch_mount() {
    // M10: the silent fail-open closes — an outlet inside the component's
    // OWN Show mounts its fallback when the branch mounts (child-side,
    // direct-rooted); nothing rendered there before.
    let src = r#"export function Inner({ flag }) {
  return (
    <Show when={flag()}>
      <Slot name="a"><em>fallback</em></Slot>
    </Show>
  );
}
"#;
    let module = lig_parse_solid::lower_module(src, "ownbranch.tsx").expect("lower");
    let text = lig_codegen::generate_module(&module).expect("codegen").text;
    assert!(
        text.contains("::lig_runtime::SlotOrigin::Fallback") && text.contains("b.mount_slot("),
        "the own-branch outlet mounts its fallback at branch mount: {text}"
    );
}

#[test]
fn list_body_components_carry_per_item_instance_identity() {
    // M13 keyed-component story: a component use on a For-body fragment
    // lowers to a per-item instance store — a `Vec<Option<Child>>` field
    // mirrored against the keyed plan ops by the reconcile method (insert
    // builds + attaches, remove takes + tears down, move repositions
    // without rebuilding), and the per-item replay drives the child's
    // props. The branch-context backstop (component in a branch INSIDE a
    // list body) stays fail-closed.
    let src = r#"import { For } from "solid-js";
export function App() {
  const [rows, setRows] = createSignal(["alpha", "beta"]);
  return (
    <main>
      <For each={rows()} key={(t) => t}>{(t) => <Badge count={t} />}</For>
    </main>
  );
}

export function Badge({ count }) {
  return <b class="badge">{count}</b>;
}
"#;
    let module = lig_parse_solid::lower_module(src, "listbody.tsx").expect("lower");
    let text = lig_codegen::generate_module(&module).expect("codegen").text;
    // The per-item instance store replaces the eager field.
    assert!(
        text.contains("pub badge: ::alloc::vec::Vec<::core::option::Option<Badge>>"),
        "list-body components are per-item instance stores: {text}"
    );
    assert!(
        text.contains("badge: ::alloc::vec::Vec::new()"),
        "the store starts empty (items mount on the first reconcile): {text}"
    );
    // Remove: take the instance, dispose its slab, then unmount the item.
    assert!(
        text.contains("if let Some(inst) = self.badge.remove(index) {"),
        "the remove step takes the per-item instance: {text}"
    );
    assert!(
        text.contains("inst.frag.teardown(h);"),
        "the removed instance's slab disposes: {text}"
    );
    // Insert: build the instance, attach its root into the item's component
    // hole, store it at the live index.
    assert!(
        text.contains("let inst = Badge::build(h, BadgeProps::default());"),
        "the insert step builds a fresh instance: {text}"
    );
    assert!(
        text.contains("self.badge.insert(index, Some(inst));"),
        "the instance stores at the live item index: {text}"
    );
    assert!(
        text.contains("h.insert(n, inst.frag.roots()[0], 0);"),
        "the fresh root attaches at the item's component hole: {text}"
    );
    // Move (keyed): the instance repositions WITHOUT rebuilding.
    assert!(
        text.contains("let inst = self.badge.remove(from);")
            && text.contains("self.badge.insert(to, inst);"),
        "the move step repositions the same instance: {text}"
    );
    // The per-item replay drives the child's props and flushes it.
    assert!(
        text.contains("if let Some(c) = self.badge[i].as_mut() {"),
        "the replay arm guards on the per-item instance: {text}"
    );
    assert!(
        text.contains("::lig_runtime::flush(c, h);"),
        "the replay flushes the child after its prop sets: {text}"
    );
}

#[test]
fn list_body_outlets_mount_their_fallback_per_item() {
    // M14 per-item slot assembly: a slot outlet on a For-body fragment
    // mounts its OWN fallback into every live item's outlet hole — the
    // per-item replay mounts idempotently (an already-mounted item skips)
    // and applies the fallback's item-bound binds to the mounted content.
    let src = r#"import { For } from "solid-js";
export function Inner() {
  const [items, setItems] = createSignal(["alpha"]);
  return (
    <ul>
      <For each={items()}>{(item) => <Slot name="a"><em>{item}</em></Slot>}</For>
    </ul>
  );
}
"#;
    let module = lig_parse_solid::lower_module(src, "listslot.tsx").expect("lower");
    let text = lig_codegen::generate_module(&module).expect("codegen").text;
    assert!(
        text.contains("f.mount_slot("),
        "the item replay mounts the fallback into the item's outlet hole: {text}"
    );
    assert!(
        text.contains("::lig_runtime::SlotOrigin::Fallback"),
        "the per-item mount origin is the outlet's own fallback: {text}"
    );
    assert!(
        text.contains("slot_fallback_mut("),
        "the fallback binds address the mounted content: {text}"
    );
    assert!(
        text.contains("c.set_text(h, 0usize, &(item))"),
        "the fallback's item-bound bind substitutes the item: {text}"
    );
}

#[test]
fn list_body_outlet_state_reads_stay_fail_closed() {
    // M14 boundary: the per-item fallback's binds may read the item/index
    // only — a state read would need the flush fan-out over live items
    // (one reaction, N items), which is the follow-up.
    let src = r#"import { For } from "solid-js";
export function Inner() {
  const [rows, setRows] = createSignal(["a"]);
  const [tag, setTag] = createSignal("x");
  return (
    <ul>
      <For each={rows()}>{(item) => <Slot name="a"><em>{tag}</em></Slot>}</For>
    </ul>
  );
}
"#;
    let module = lig_parse_solid::lower_module(src, "listslot_state.tsx").expect("lower");
    let err = lig_codegen::generate_module(&module)
        .expect_err("state-reading per-item fallback binds stay denied");
    assert!(
        err.to_string().contains("per-item slot content"),
        "the precise reason names the fan-out follow-up: {err}"
    );
}

#[test]
fn list_body_outlet_fills_stay_fail_closed() {
    // M14 boundary: a use-site fill for a child's list-body outlet names no
    // outlet the parent's init walk can reach (items do not exist at build
    // time) — the M9 unmatched-fill backstop keeps it fail-closed; the
    // cross-component fill coordination is the follow-up.
    let src = r#"import { For } from "solid-js";
export function App() {
  return (
    <Panel row={<b>filled</b>} />
  );
}

export function Panel() {
  const [items, setItems] = createSignal(["a"]);
  return (
    <ul>
      <For each={items()} key={(item) => item}>{(item) => <Slot name="row" />}</For>
    </ul>
  );
}
"#;
    let module = lig_parse_solid::lower_module(src, "listslot_fill.tsx").expect("lower");
    let err = lig_codegen::generate_module(&module)
        .expect_err("fills for list-body outlets never mount — fail-closed");
    assert!(
        err.to_string().contains("the fill would never mount"),
        "the unmatched-fill backstop fires: {err}"
    );
}

#[test]
fn list_body_slot_content_stays_scalar() {
    // M14 boundary: per-item slot content admits scalar binds only —
    // events/refs/structural content inside it keeps the codegen backstop
    // (per-item dispatch and refs need the keyed-component follow-ups).
    let src = r#"import { For } from "solid-js";
export function Inner() {
  const [items, setItems] = createSignal(["alpha"]);
  const [tag, setTag] = createSignal("x");
  return (
    <ul>
      <For each={items()}>
        {(item) => (
          <Slot name="a">
            <em onClick={() => setTag("y")}>{item}</em>
          </Slot>
        )}
      </For>
    </ul>
  );
}
"#;
    let module = lig_parse_solid::lower_module(src, "listslot_event.tsx").expect("lower");
    let err = lig_codegen::generate_module(&module)
        .expect_err("events inside per-item slot content stay denied");
    assert!(
        err.to_string().contains("per-item slot content"),
        "the precise reason: {err}"
    );
}

#[test]
fn list_body_components_in_branches_stay_fail_closed() {
    // M13 boundary: a component use inside a BRANCH hanging on a list body
    // keeps the per-mount backstop (per-item identity covers the DIRECT
    // item-body use; branch-in-item composition remains the follow-up).
    let src = r#"import { For } from "solid-js";
export function App() {
  const [rows, setRows] = createSignal(["alpha"]);
  return (
    <main>
      <For each={rows()}>
        {(t) => (
          <Show when={true}>
            <Badge count={t} />
          </Show>
        )}
      </For>
    </main>
  );
}

export function Badge({ count }) {
  return <b>{count}</b>;
}
"#;
    let err = lig_parse_solid::lower_module(src, "branch_in_list.tsx")
        .expect_err("the frontend denies branch-context components inside list bodies");
    assert!(
        err.iter().any(|d| d.code == "LIG-E001"),
        "the sticky in-for contract fires: {err:?}"
    );
}

#[test]
fn template_ref_generates_the_node_field_and_capture() {
    // M12: the Vue template-ref spelling rides the existing Ref machinery —
    // the generated unit declares the sentinel-initialized `ref_<name>`
    // field and captures the element's hole NodeId in the build body.
    let src = r#"<script setup lang="ts">
defineOptions({ name: "Marked" });

let box;
</script>

<template>
  <section>
    <input ref="box" />
    <p>static</p>
  </section>
</template>
"#;
    let module = lig_parse_vue::lower_module(src, "template-ref.vue").expect("lower");
    let text = lig_codegen::generate_module(&module).expect("codegen").text;
    assert!(
        text.contains("pub ref_box: ::lig_runtime::NodeId"),
        "{text}"
    );
    assert!(
        text.contains("ref_box: ::lig_runtime::NodeId(u32::MAX)"),
        "the field starts as the absent sentinel: {text}"
    );
    assert!(
        text.contains("s.ref_box = s.frag.hole("),
        "the build body captures the element hole: {text}"
    );
}

#[test]
fn template_ref_codegen_matches_the_solid_leg() {
    // The two spellings ride one Ref contract — the generated text is the
    // same modulo the source-file identity the doc comment carries.
    let vue = r#"<script setup lang="ts">
defineOptions({ name: "Marked" });

let box;
</script>

<template>
  <section>
    <input ref="box" />
  </section>
</template>
"#;
    let solid = r"export function Marked() {
  let box;
  return <section><input ref={box} /></section>;
}
";
    let vue_module = lig_parse_vue::lower_module(vue, "template-ref.vue").expect("lower");
    let solid_module = lig_parse_solid::lower_module(solid, "template-ref.tsx").expect("lower");
    let vue_text = lig_codegen::generate_module(&vue_module)
        .expect("codegen")
        .text;
    let solid_text = lig_codegen::generate_module(&solid_module)
        .expect("codegen")
        .text;
    let source_line = |t: &str| {
        t.lines()
            .filter(|l| l.contains("template-ref."))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let strip = |t: &str| t.replace(&source_line(t), "");
    assert_eq!(
        strip(&vue_text),
        strip(&solid_text),
        "one Ref contract, one generated shape"
    );
}

#[test]
fn else_branch_mounts_unmounts_and_swaps() {
    // M11 — the else leg: a Show with a fallback emits BOTH legs' mount
    // statics and a leg tracker; the cond arm mounts the else branch when
    // the condition is false (the M2-era fail-open mounted `then` only and
    // left the emitted else template dead). The then-only shape stays
    // byte-stable; the else shape carries one `mount_branch` per leg.
    let src = r#"export function App() {
  const [on, setOn] = createSignal(true);
  const [tag, setTag] = createSignal("in");
  return (
    <Show when={on()} fallback={<i class="off">{tag}</i>}>
      <b class="on">{tag}</b>
    </Show>
  );
}
"#;
    let module = lig_parse_solid::lower_module(src, "else.tsx").expect("lower");
    let text = lig_codegen::generate_module(&module).expect("codegen").text;
    assert_eq!(
        text.matches(".mount_branch(").count(),
        2,
        "one mount per leg (then + else): {text}"
    );
    assert!(
        text.contains("leg_f"),
        "the else shape tracks the mounted leg: {text}"
    );
}

#[test]
fn else_bind_arms_address_their_branch() {
    // M11: a bind on the else branch walks the branch ctx like its then
    // twin — `branch_owner_of` must classify else fragments as branches, or
    // the else bind mis-addresses `self.frag` (the Direct mis-classification
    // that made dynamic else content corrupt instead of render).
    let src = r#"export function App() {
  const [on, setOn] = createSignal(true);
  const [tag, setTag] = createSignal("in");
  return (
    <Show when={on()} fallback={<i class="off">{tag}</i>}>
      <b class="on">{tag}</b>
    </Show>
  );
}
"#;
    let module = lig_parse_solid::lower_module(src, "elsebind.tsx").expect("lower");
    let text = lig_codegen::generate_module(&module).expect("codegen").text;
    assert_eq!(
        text.matches("if let Some(b) = self.frag.branch_mut(0usize)")
            .count(),
        2,
        "both legs' binds guard on the mounted branch: {text}"
    );
    assert_eq!(
        text.matches("b.set_text(h, 0usize").count(),
        2,
        "both legs' binds write through the branch: {text}"
    );
}

#[test]
fn else_branch_components_rebuild_per_mount() {
    // M11: M10 per-mount identity generalizes to the else leg — a component
    // use on the else branch builds as an Option field, rebuilds when the
    // else branch mounts, disposes when it unmounts, and the prop-pass arm
    // guards on the instance. Distinct child types keep the two legs'
    // fields unambiguous (a shared child would make the field naming
    // decide the test).
    let src = r#"export function App() {
  const [on, setOn] = createSignal(true);
  const [tag, setTag] = createSignal("two");
  return (
    <div>
      <Show when={on()} fallback={<Tag note="off" />}>
        <Badge note={tag} />
      </Show>
    </div>
  );
}

export function Badge({ note }) {
  return <em class="badge">{note}</em>;
}

export function Tag({ note }) {
  return <i class="tag">{note}</i>;
}
"#;
    let module = lig_parse_solid::lower_module(src, "elsecomp.tsx").expect("lower");
    let text = lig_codegen::generate_module(&module).expect("codegen").text;
    // The else-branch use is a per-mount Option field (eager today — the
    // else use classifies Direct and builds at build time, the M10
    // stale-eager-instance shape in a new location).
    assert!(
        text.contains("pub tag: Option<Tag>"),
        "the else-branch use is a per-mount Option field: {text}"
    );
    assert!(
        text.contains("self.tag = Some(Tag::build(h, TagProps::default()));"),
        "the else mount rebuilds the instance: {text}"
    );
    assert!(
        text.contains("self.tag.take()"),
        "the unmount path resets the else instance: {text}"
    );
    assert!(
        text.contains("if let Some(c) = self.tag.as_mut()"),
        "the prop-pass arm guards on the else instance: {text}"
    );
    // The then use stays eager-Direct-free: the THEN branch's use is a
    // Branch-ctx Option too (M10), with its own rebuild arm.
    assert!(
        text.contains("pub badge: Option<Badge>")
            && text.contains("self.badge = Some(Badge::build(h, BadgeProps::default()));"),
        "the then-branch use keeps its M10 per-mount shape: {text}"
    );
}

#[test]
fn else_branch_outlets_mount_fills_with_the_branch() {
    // M11: the M10 branch-time slot assembly generalizes to the else leg —
    // an outlet inside the else branch mounts its use-site fill when the
    // else branch mounts (author-side content; the ctx map already walked
    // else branches, only the mounting arm ignored them).
    let src = r#"export function App() {
  const [on, setOn] = createSignal(true);
  const [tag, setTag] = createSignal("in");
  return (
    <Panel
      title="x"
      header={
        <Show when={on()} fallback={<Slot name="inner"><em>off</em></Slot>}>
          <Slot name="inner"><em class="none">{tag}</em></Slot>
        </Show>
      }
      inner={<b class="hl">{tag}</b>}
    />
  );
}

export function Panel({ title }) {
  return <section><Slot name="header" /></section>;
}
"#;
    let module = lig_parse_solid::lower_module(src, "elseslot.tsx").expect("lower");
    let text = lig_codegen::generate_module(&module).expect("codegen").text;
    // 3 = the build-time header fill mount + one branch-time mount per leg.
    assert_eq!(
        text.matches("b.mount_slot(").count(),
        2,
        "the assembly runs per leg (plus the build-time fill mount): {text}"
    );
    assert_eq!(
        text.matches("::lig_runtime::SlotOrigin::Fill").count(),
        3,
        "both legs mount their use-site fill (plus the build-time mount): {text}"
    );
}

#[test]
fn own_else_branch_outlets_mount_their_fallback() {
    // M11: the child-side mirror of M10's own-branch fallback mount — an
    // outlet inside the component's OWN else branch mounts its fallback
    // when the else branch mounts.
    let src = r#"export function Inner({ flag }) {
  return (
    <Show when={flag()} fallback={<Slot name="a"><em>off</em></Slot>}>
      <Slot name="a"><em>on</em></Slot>
    </Show>
  );
}
"#;
    let module = lig_parse_solid::lower_module(src, "ownelse.tsx").expect("lower");
    let text = lig_codegen::generate_module(&module).expect("codegen").text;
    assert_eq!(
        text.matches("b.mount_slot(").count(),
        2,
        "fallback mounts on both legs: {text}"
    );
    assert_eq!(
        text.matches("::lig_runtime::SlotOrigin::Fallback").count(),
        2,
        "both legs mount their own fallback: {text}"
    );
}
