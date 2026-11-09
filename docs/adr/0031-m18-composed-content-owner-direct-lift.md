# ADR-0031: M18 — the composed-content owner-direct lift

- Status: accepted (M18)
- Deciders: compiler team
- Date: 2026-11-09

## Context

ADR-0030's decision class — "events directly on a composed `For` body
admit" — named composed bodies as *branch- or slot-owned*, but only the
branch half ever lowered. The slot-content half denied at the FRONTEND: the
content contract's walk (`content_contract`, both legs) kept two sticky
arms from the pre-M15 era — every `Event` and every `Slot` dynamic on a
fragment marked `in_list_item` denied with `LIG-E001`, regardless of depth.
Three docs claimed the slot half already: the LIG-E118 catalog entry ("a
For whose owner hangs on a branch or inside slot content"), ADR-0030's own
"slot-owned bodies are one admission class", and the `composed_events`
example's doc header. The code lagged the documented contract — the docs
were drift on this tree, and the drift was discoverable by one `lig check`
probe.

The premise the content denial once relied on is gone. M14's per-item slot
assembly emits its mount through the SHARED per-item replay
(`apply_item_f*` → `emit_item_slot_arm`) — a `&mut Fragment` method that
never touches the owner, so it is receiver-neutral by construction. M17's
composed reconcile attaches per fresh item through any receiver
(`item_event_attaches(recv, …)`). M14's reaction filtering scans the module
for For-body outlet fallbacks with no ctx knowledge. The fill-carried list
reconciles through the composed walk since M8
(`composed_contexts_address_content_branches_and_lists` pins it). What
remained was exactly two frontend arms and nothing else.

## Decision

### 1. The composed list body's OWN surface admits on content, one level deep

The content walk grows an `owner_direct` flag beside `in_list_item`: the
fragment a `For` dynamic hangs off (the body) admits its own events and
outlets; every fragment deeper in the item subtree keeps the denial. The
flag is computed from an un-reset `in_item_subtree` bit — branch bodies
still reset `in_list_item` (the pre-M18 shape; admission's E118 walk and
the own-tree contract catch those deeper shapes), but they preserve the
subtree bit so a NESTED list body never gains the owner-direct surface.
One mechanism, two constructs: the event arm narrows to the E118 reason,
the outlet arm to the branch-in-item reason.

### 2. Zero codegen, runtime, and admission changes — for the lift itself

The shared replay, the composed reconcile, and the reaction filtering carry
the surface unchanged: the codegen pin test
(`composed_content_list_bodies_carry_the_per_item_surface`) passed on the
same commit that lifted the walk. The generated shape is the M17 machinery
with a content receiver.

### 3. The M18 fixture exposed — and closed — a latent M10 fail-open

Compile-clean refused the fixture's generated unit: a fill authored at a
BRANCH-mounted component use emitted `s.<field>.frag.mount_slot(…)` in the
build body, where the per-mount instance is `None` until the branch mounts
(M10 identity). The build-phase slot-assembly loop never filtered per-mount
uses, and no ctx carrying a fill knew the field was an `Option`. The fix is
two-sided, in the same boundary's spirit (fills INTO branch-mounted
instances are the per-mount complement of the branch-time assembly):

- the build-phase loop skips `Ctx::Branch` uses — their fills mount in the
  mounting arm;
- the mounting arm (`leg_rebuilds`) mounts the use-site fills into the
  FRESH instance after the rebuild + root insert, attaches the content's
  events per mount (fill records on the parent's slab, fallback records on
  the child's — the M7 ownership discipline), init-flushes fallback binds
  on the instance, and widens the mount mask by the fill content's
  non-handler reactions so a fill-carried list's reconcile re-runs into the
  fresh instance (the M17 mount-mark story, composed with M10).

`Ctx::Fill` grows a `per_mount` flag (computed by an IR-global
`owner_on_branch` scan — fragment ids belong to one component tree), and
the three ctx emitters (`ctx_scope`, `ctx_frag_read`, `ctx_frag_mut`) guard
their `base = Direct` unwraps on it. Direct-fill emission is byte-stable:
every pre-existing unit regenerates unchanged, and `lig reproducibility`
still reports the counter byte-identical.

### 4. Fail-closed boundaries, redrawn one step — again

Events beyond the composed content body (item branches, nested lists) keep
the denial; outlets beyond it keep theirs; fills for list-body outlets keep
the M9 backstop; per-item slot content still admits scalar binds only; and
state-reading fallback binds keep the codegen backstop. The e118 allow
fixture redraws to the composed-content shape (the new boundary), the deny
case stays branch-in-item, and the rule row + the E118 doc move with it.

## Consequences

- The `for-composed-content` fixture (both legs) pins the shape: one IR,
  byte-equal legs, the fill-carried composed reconcile, the per-item
  attach, the per-item fallback mount. The demo test pins the runtime
  story: fill mounts into the fresh instance at branch mount, dispatch
  through the fire fn, keyed moves carry records + mounted content, the
  branch cycle rebuilds everything per mount.
- The `codegen_for_composed_content` ledger row joins the §14.2 budget
  ledger (release median 1251 µs against a 6 500 µs ceiling, ≈5.2×
  headroom); 20 rows, all enforced in release.
- The consumer surface gains a fourth runnable example: `composed_content`
  — the admitted composed-content unit end to end, the generated surface,
  and the still-denied zone rendered through miette.
- Branch-in-item and nested-list composition remain the named foundation
  follow-ups; fills-with-refs into per-mount instances keep the M10-family
  guard gap (fail-loud: the generated unit refuses to compile, the
  compile-clean gate catches it before anything ships).
