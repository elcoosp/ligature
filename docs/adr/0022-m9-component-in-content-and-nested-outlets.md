# ADR-022: M9 — component-in-content (grandchild prop-pass) and nested outlets (composed fill contexts)

- Status: accepted (M9)
- Deciders: compiler team
- Date: 2026-11-08

## Context

M8's known gap: "Component uses inside slot content and nested outlets
inside slot content: fail-closed (`LIG-E001`). Component-in-content needs
grandchild prop-pass THROUGH mounted instances; nested outlets need
composed fill contexts (a `Fill` base field and per-instance addressing)."
ADR-021 named the extension point: "The `Ctx` recursion is the extension
point for the next milestone: a `Fill`/`Fallback` base field arrives with
composed fill contexts if nested outlets in content are ever lifted." M9
delivers both halves — the Component/Slot half of the same gap whose
If/For half M8 closed.

## Decision

### 1. No IR change, no runtime change

Both constructs are already IR-expressible (`DynamicKind::Component` and
`DynamicKind::Slot` on any fragment; schema stays v5), and the runtime
already mounts content as full `Fragment`s. The one new runtime-visible
commit is the grandchild's DOM root entering the mounted content — and
that rides the EXISTING `Host::insert`: the commit stream is the only
tree-mutation surface, so a foreign root sliding into a content hole is
the same primitive `mount_slot` already uses. No new runtime concept, no
new error code, no dependency.

### 2. Grandchild prop-pass through mounted instances

A component use inside content keeps the Direct-context ownership shape:
the grandchild instance is a struct field of the CONTENT AUTHOR (the use
walks the whole tree — content fragments are tree children — and the
per-use field suffixing is unchanged). What the mounted content adds is a
MOUNT-TIME story, the M7/M8 discipline applied to instances:

- **Insert at mount** — the parent's slot assembly, after mounting a
  content, inserts each grandchild's root into the content's component
  hole (`h.insert(content.hole(n), s.<field>.frag.roots()[0], 0)`),
  through the composed read walk. Content mounts exactly once (the
  once-only guard), so the insert is once-only by construction.
- **Prop-pass is ctx-independent** — the `Component` run arm writes the
  grandchild's setters and flushes the instance; it never touches the
  content walk (the instance is the author's field). The prop-pass bind
  is a reaction owned by the content fragment (in the author's tree), so
  the exact build mask covers its initialization, and the initial flush
  runs AFTER slot assembly — props land on a mounted grandchild.
- **Nested authorship** — a component use inside a CHILD-authored
  fallback belongs to the child's tree: its instance is the child's
  field, its prop-pass arm is the child's reaction, and the PARENT (which
  mounts the fallback) performs the insert addressing the child's field.
  The walk generalizes the M7 owner-prefix rule: statics and fields
  follow the tree that contains the fragment.

Components inside BRANCH or LIST bodies of content stay fail-closed
(precise reason): a remounted branch would re-insert a root the previous
teardown already destroyed — instances need per-mount identity (rebuild
per mount), not re-insertion. Per-item component instances remain the M2
keyed-component story.

### 3. Composed fill contexts

`Ctx::Fill` grows a base field and an accessor selector:
`Fill { field, slot, fill, base: Box<Ctx> }` — `field` names the child
instance, `slot` the outlet's dynamic ordinal, `fill` selects
`slot_fill_mut` vs `slot_fallback_mut` (a fill matched to an outlet that
lives inside the child's FALLBACK content addresses it through the
fallback accessor), and `base` is the context of the fragment the outlet
hangs on. `Ctx::Fallback` grows the same base for the child's own nested
fallbacks. `ctx_scope`/`ctx_frag_read` compose base-first; `base = Direct`
reproduces the M7 emission byte-for-byte (the existing shapes are
untouched). The composed read walk (`ctx_frag_read`) drives the mount-time
attaches, inserts, and ref captures without holding a mutable unwrap.

`build_content_ctx` becomes a two-sided walk:

- **Child side** — for each use site, walk the child's ROOT for outlets
  (base `Direct`, the M7 shape), then recurse into the child's own
  fallback contents for deeper outlets (base the composed `Fallback`
  chain). Branch/For fragments of the child are NOT walked — outlets
  inside them mount with the branch, which needs branch-mount-time fill
  coordination (deferred, precise codegen denial).
- **Author side** — walk the use site's OWN fill contents for nested
  outlets (an outlet the author spelled inside its fill): fills of the
  same use site match by name, and the ctx composes
  `Fill { base: Fill { .. } }`.

A use-site fill that matches NO outlet on either side is a codegen
fail-closed error — closing the M6-era silent-drop fail-open (a fill
naming an outlet the child does not declare simply never mounted).

### 4. The recursive content walk in the slot assembly

The slot assembly's per-outlet mount becomes a depth-first walk over the
mounted content: nested outlets mount their use-site fill
(`SlotOrigin::Fill`) or the outlet's own fallback (`SlotOrigin::Fallback`)
through the composed accessor chain, then the walk recurses into the
freshly mounted content for ITS outlets and component holes. Statics and
field prefixes follow the tree-membership rule; nested fallback content
joins the fallback init-flush subtree (`subtree_ids` already recurses
through the children chain), so its binds initialize inside the
parent-driven flush, after the walk mounts everything. Events on nested
content attach at mount with the record on the AUTHOR's fragment (tree
membership decides parent vs child); nested-content refs capture at mount
into the author's field.

### 5. The M9 content contract (frontends)

`Component` and `Slot` dynamics admit DIRECTLY on fill/fallback content —
and on the Vue leg's content ROOTS (root `<slot>` tags and root component
tags dispatch through `fragment_from_content_root` now; the M8 denials
there existed because `lower_element` does not dispatch the tag — the
same fail-open shape the root-`<slot>` check closed by denying). The
contract walk carries the structural position: `in_branch` (an If branch
or For body of content) and `in_list_item` (a For body). Fail-closed with
one precise reason each: Component/Slot inside branch or list bodies of
content (per-mount / per-item instance identity), children/prop-pass/
whole-style dynamics (M2), event handlers inside For bodies (the
LIG-E118 discipline).

## Consequences

- Fills and fallbacks compose the full component surface: grandchild
  components with reactive prop-pass, and outlets whose own fills and
  fallbacks mount with their content.
- `Ctx` now composes over every mounted-content shape the runtime has:
  `Branch { base }`, `Fill { base }`, `Fallback { base }` — the ADR-020
  flat-enum caveat ("a branch can only hang on the component's own root")
  is fully retired.
- The fill-matching backstop turns the silent drop of unmatched fills
  into a compile error (fail-closed, BR-1).
- The remaining content gap is structural: components and outlets inside
  branch/list bodies of content (per-mount/per-item identity), outlets
  inside the child's own branch fragments (branch-mount-time fill
  coordination), `v-model` on non-input elements, `<slot :name>`
  forwarding, and template refs on the Vue leg.
- The counter unit is untouched (no slots, no component uses) — the
  digest and the reproducibility gate are unchanged; the new shapes ride
  a new codegen bench workload with a ledger row (§14.2).
