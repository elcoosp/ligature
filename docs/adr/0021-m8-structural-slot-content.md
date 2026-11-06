# ADR-021: M8 — structural slot content (branch-in-content, list-in-content, composed `Ctx`)

- Status: accepted (M8)
- Deciders: compiler team
- Date: 2026-11-06

## Context

M7's known gap: "If/For/Component/Slot inside slot content: fail-closed
(`LIG-E001`). Branch-in-content and list-in-content need content-scoped
branch/list tables or composed `Ctx` contexts; component-in-content needs
grandchild prop-pass through mounted instances." M7's `Ctx` was flat —
`Branch { slot }` with an implicit `Direct` base — so an If/For on a fill
or fallback had nowhere to mount: the arm would have addressed the
component's own fragment, not the content. ADR-020 named the shape the
enum was ready to grow ("a base field, then").

## Decision

### 1. No runtime change: content was already content-scoped

The mounted content is a full `Fragment` with its own branch and list
tables (`mount_slot` instantiates it from the authoring tree's statics);
`slot_fill_mut`/`slot_fallback_mut` return it. Branch-in-content and
list-in-content were a CODEGEN addressing gap, not a runtime one — no new
runtime concept, no new error code, no dependency, IR v5 unchanged.

### 2. The composed `Ctx`

`Branch { slot, base: Box<Ctx> }` composes over the parent fragment's
context; `Fill`/`Fallback` stay leaves (they always hang off the component
struct's own child field, never inside a walk). `ctx_of` recurses — a
branch wrapper first (over the If/For dynamic's owner fragment), then the
content map, else `Direct` — so arbitrary chains classify:
`Branch { base: Fill }` (branch-in-content),
`Branch { base: Branch { base: Direct } }` (nested Ifs). `ctx_scope`
unwraps outermost-first with shadowing receivers (`b`/`c`), so
`ctx_recv` is always the innermost binding. A read-only walk
(`ctx_frag_read`) yields `Option<&Fragment>` for the mount-time attaches
and captures without holding the mutable unwrap open.

The composition closes two M7-era latent holes the flat enum could not
express: a bind on an inner branch of NESTED Ifs addressed
`self.frag.branch_mut(inner)` (the outer level was unreachable), and the
For reconcile method hard-addressed `self.frag` (a For inside a branch
reconciled the wrong table). Both now walk the composed chain.

### 3. The mount-time story for content-rooted branches

A branch on slot content mounts and unmounts with its content, so its
per-mount work lives in the mounting If arm, not the build body:

- **Init marking** — mounting marks the fresh branch SUBTREE's non-handler
  reactions (`subtree_init_mask`) on the component's dirty set; the flush
  loop re-enters through `produced.union(&rx.dirty().take())` and
  initializes the branch's binds on the next pass. Without it, a branch
  bind whose reads are disjoint from its cond would never run on a late
  mount (the setter marks only the cond's readers). Direct-rooted branches
  keep the M0/M7 arm shape (their co-read case is the pinned behavior; the
  general re-init is content-scoped).
- **Event attaches** — the branch fragment's events attach across slabs at
  mount (`attach_foreign`; the record lands on the authoring component's
  fragment, dispatch marks it, the author's flush runs the reaction).
- **Ref captures** — content-branch refs capture per mount through the
  read walk (`ref_x = branch.hole(n)`), not once at build.

### 4. For arms through the composed walk

Direct-rooted lists keep the M0 `&mut self` method shape (byte-stable).
Composed lists emit a receiver-param ASSOCIATED fn:
`Self::reconcile_fN(h, recv, slot, items, s_a, …)` — the run arm
pre-clones the source and the parent-state reads, unwraps the composed
walk, and calls the associated fn. Taking no `self` keeps the mutable walk
borrow and the value reads non-overlapping; the key fn and the per-item
bind replay (`apply_item_fN`) are shared verbatim with the Direct shape.

### 5. The fallback init-flush goes subtree-wide

With structural fallbacks, the fallback's branch/list fragments own
reactions too (the If arms that mount the content's own branches, the
branch binds). The parent's exact per-reaction mask now covers the whole
fallback SUBTREE (`subtree_ids`), event handlers excluded by id — the M7
mask discipline applied to the subtree.

### 6. The M8 content contract (frontends)

If/For inside fills and fallbacks are ADMITTED at any depth — the content
contract walk recurses through branch/body fragments (scalar binds,
events, refs, and further structural constructs all compose). Event
handlers inside content For bodies stay denied (the LIG-E118 discipline).
Still fail-closed, one reason each (`LIG-E001`): component uses in content
(grandchild prop-pass through mounted instances), nested outlets in
content (composed fill contexts), children/prop-pass/whole-style (M2).

The Vue leg admits `v-if`/`v-for` on content ROOTS:
`fragment_from_content_root` lowers the structural root ONTO the content
fragment (the M7 strip-and-deny existed because `fragment_from_elem`
silently DROPPED the directive — the M6-era fail-open shape).
`v-else-if`/`v-else` roots have no chain to join (denied; put the chain on
the content's children), and root component tags / root `<slot>` tags stay
denied (the root-`<slot>` check also closes a fail-open: `lower_element`
does not dispatch the tag, so a nested outlet silently lowered as a plain
element). The Solid leg's structural roots were already expressible
(`header={<Show …>}` lowers onto the fill fragment); both legs now admit
the same shapes.

## Consequences

- Fills and fallbacks are full program surfaces: any admitted fragment
  body (binds, events, refs, If/For) works inside slot content, addressed
  by one recursive discipline instead of special cases.
- Component-in-content (grandchild prop-pass) and nested outlets in
  content remain the known gap — they need prop-pass THROUGH mounted
  instances and composed fill contexts, not branch walks.
- The mount-time marking adds one redundant pass at build for
  initially-mounted content branches (the bind runs in the init mask AND
  the mount mark) — idempotent binds make the extra pass invisible.
- The `Ctx` recursion is the extension point for the next milestone: a
  `Fill`/`Fallback` base field arrives with composed fill contexts if
  nested outlets in content are ever lifted.
