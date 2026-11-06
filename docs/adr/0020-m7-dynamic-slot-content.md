# ADR-020: M7 — dynamic slot content (slot-scoped addressing, the ref-capture split, exact build assembly)

- Status: accepted (M7)
- Deciders: compiler team
- Date: 2026-11-06

## Context

M6's static-subtree slot-content contract ("fills and fallbacks mount as
static subtrees — the runtime has no run arms addressing a slot
instance's holes, so dynamic binds inside slot content would write
nowhere") denied every dynamic bind, event, and ref inside slot content,
and both frontends fail-closed on it with `LIG-E001`. The M6 report named
the two prerequisites for lifting it: slot-scoped run arms and an
init-flush story for mounted slot instances. M7 delivers both, plus the
build-assembly work they expose.

## Decision

### 1. One runtime concept: `SlotOrigin`

A mounted slot outlet stores a `SlotInstance { origin: SlotOrigin, anchor,
root, frag }` instead of a bare `BranchInstance`. `SlotOrigin` is `Fill`
(parent-authored content, parent-owned reactions) or `Fallback`
(child-authored content, child-owned reactions). Accessors are
origin-gated — `slot_fill`/`slot_fill_mut` return the fragment only for
`Fill`, `slot_fallback`/`slot_fallback_mut` only for `Fallback` — so a
fallback reaction writing into a fill (or vice versa) is unrepresentable
rather than a runtime check. `mount_slot` takes the origin; `slot_is`
keeps its meaning (any content mounted). Disposal, anchoring, and the
once-only guard are unchanged.

### 2. One addressing discipline in codegen: `Ctx`

Every generated run arm classifies its owner fragment once into a `Ctx`:

- `Direct` — the component's own fragment; receiver `self.frag` (the M0
  shape, byte-stable);
- `Branch { slot }` — a mounted If branch; receiver the branch fragment
  (previously only Text had this special case; every other kind wrote to
  `self.frag` regardless of where the dynamic lived);
- `Fill { field, slot }` — a use-site fill; receiver the content
  fragment on the child instance's fragment;
- `Fallback { slot }` — the outlet's own fallback; receiver the content
  fragment on `self.frag`.

The classification comes from a build-time map over the component's use
sites (fills) and own outlets (fallbacks), falling back to the If/For
walk, else `Direct`. `Ctx` is flat: slot content is denied If/For (§4
below), so a branch can only hang on the component's own root. The
branch-slot lookup becomes the owner dynamic's If/For ordinal — the raw
children position was the branch table index only while branch fragments
were the only tree children (M6's fill fragments broke that identity).

### 3. Events and refs cross slabs; the parent owns mount time

The runtime gains `attach_foreign(node, ev, handler, delegate, host)`:
the handler record lands on the CALLING fragment while the node may live
in another fragment's slab — dispatch marks the recording fragment's
dirty set (handler id = reaction id, §10.1), so the owning component's
flush runs the reaction.

The parent's build owns the whole mount-time story (it is the only place
both origins exist): mount → attach the content's events (record on the
parent for fills, on the child instance for fallbacks) → capture the
content's refs → for a mounted fallback, mark the child's fallback
reactions and flush the child (the init-flush story — the child's build
flushed before the content was mounted). Event-driven handler reactions
are excluded from the fallback mask by id; the build body phases are:
slot assembly → direct ref captures → direct attaches → initial flush →
branch attaches + branch ref captures.

### 4. The ref-capture split

`ref={el}` captures the hole NodeId of the fragment the ref lives on.
Four capture phases: direct refs in the build body (byte-stable M0
shape), fill refs through the mounted content, fallback refs through the
child's outlet (the parent writes the child's `pub` field — the fallback
fragment belongs to the child's tree), branch refs after the initial
flush mounts the branch. Ref fields keep the absent sentinel
`NodeId(u32::MAX)` until their content exists. The Vue template has no
`ref` spelling yet; the split is pinned on the Solid leg by a codegen
contract test covering all four contexts.

### 5. Exact per-member build masks

The initial-flush mask is exact: the bit IS the global reaction id. The
M0-era `(1 << n) - 1` width band marked the wrong component's ids for
every component after the first in a module and missed its own late ids
— invisible while slot content was static, fatal for the fallback
init-flush (which needs exact bits). Script effects still run at build
(`effect_multi` pins it); only event-driven handler reactions are
excluded — their bits are exactly the dispatch set
(`dispatch_lo`/`dispatch_hi`), applied as the mask's complement.

### 6. The M7 slot-content contract (frontends)

Scalar binds (Text/Class/Attr/StyleProp/Html), Event dynamics, and Ref
dynamics directly on fill/fallback fragments are admitted. Structural
constructs stay fail-closed with one reason each (`LIG-E001`): If/For
(branch/list tables hang on `self.frag`), Component (grandchild prop-pass
crosses into a mounted instance), nested Slot (needs composed contexts).
Two M6-era fail-opens closed on the Vue leg: `fragment_from_elem`
silently STRIPPED structural directives on fill/fallback roots (the
static-content check ran after the strip, so `v-if` was erased and the
module accepted), and component tags on content roots lowered as plain
elements (the use vanished). Both now deny before the strip/lowering.

## Consequences

- The generated unit's build fn carries the phase documentation (the
  counter unit grows 7 146 → 7 437 B against the 8 192 B ceiling; the
  component digest is unchanged — no IR change, schema stays v5).
- Composed addressing contexts (branch-in-content, list-in-content) are
  the next milestone's surface: `Ctx` grows a base field, content-scoped
  branch/list tables follow the same mount-time pattern.
- The parity gate is unchanged (24 fixtures, both legs); the admission
  ladder is unchanged (22 cases, 11 rules — the new denials reuse
  `LIG-E001`).
