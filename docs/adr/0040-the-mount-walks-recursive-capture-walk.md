# ADR-0040: The mount walk's recursive capture walk

- Status: accepted
- Deciders: compiler team
- Date: 2026-11-26

## Context

The per-mount fill refs (M25, ADR-0039) routed the TOP-LEVEL fill's ref
captures into the branch mounting arm's leg walk — and left one named
follow-up in the M25 report's gap list: **refs inside nested slot content of
a branch-mounted use** still denied with a precise codegen error, "until the
mount walk grows a recursive capture walk (the same recursion the M9
nested-outlet mounts gained)".

Probing the boundary showed the gap was wider than the capture alone. For a
branch-mounted use whose fill carries a nested outlet, the M18 leg walk
mounted only the TOP-LEVEL fill into the fresh instance — nothing ever
mounted the nested outlet's own fill or fallback:

- the nested fill content never entered the tree, so the composed bind run
  arms (`slot_fill(header)` → `slot_fill(inner)`) addressed content that did
  not exist — a silent fail-open, the M6-era shape in a new location;
- a ref inside that nested fill denied at codegen (fail-closed, M25);
- the child's own root-outlet fallback content, mounted per mount by the
  leg walk's fallback branch, had no ref capture either — its `ref_` fields
  stayed at the `NodeId(u32::MAX)` sentinel forever (a second silent
  fail-open).

The build-phase slot assembly (direct uses, M9) already had the full
recursion: `walk_content_mounts` mounts nested outlets' fills and own
fallbacks, inserts grandchild roots, attaches nested events and captures
nested refs — all through ONE walk. The branch-time assembly (M10) reuses
the same walk for outlets on branch legs. Only the per-mount leg walk
(M18's fill-mount block) inlined its top-level story without the recursion.

## Decision

### 1. The leg walk recurses through the shared content walk

The M18 per-mount fill mount (and its fallback twin) calls
`walk_content_mounts` on the freshly mounted content, per mount: nested
outlets mount their use-site fills or own fallbacks, grandchild roots
insert, nested events attach, nested refs capture — the M9 recursion, now
on the mount path. The walk's receivers are composed by the caller as the
`Option<&Fragment>` / `Option<&mut Fragment>` expressions of the walked
content, so the per-mount entry passes the guarded instance chains
(`self.<field>.as_ref().and_then(|inst| inst.frag.slot_fill(<outlet>))`)
and every deeper level chains one more `and_then` — the M18 guard
discipline rides the whole recursion.

### 2. The author spelling is a walk parameter

The walk's statements address the content's AUTHOR: the build body binds
`s` (the struct under construction), a mounting arm addresses `self`, and a
per-mount child-authored statement guards on the `Option<Child>` field
first and writes the CHILD instance's field through `inst` (the M7
fallback-ref ownership, per mount). This also fixes the branch-time
assembly's ref captures, which spelled `s.` in run-arm context where `s`
does not exist — a shape no committed unit exercised (it would not
compile), now spelled correctly.

### 3. Nested fills join the mount mask

Nested fill contents are children of the use site's owner fragment, NOT of
the content they mount into, so the M18 widening
(`subtree_init_mask(content_id)`) misses their reactions. The walk reports
every fill-origin content it mounts and the caller widens the mount mask by
each — scoped to per-mount contexts so every unit without per-mount fills
stays byte-identical (D-8; the same scoping discipline M17 used for
list-carrying branches).

### 4. The denial lifts for the covered chains

The M25 codegen denial (`Ctx::Fill { per_mount: true, base }` with a
non-`Direct` base) lifts when the base chain routes to a mount walk the
mounting arms emit: every base link is a per-mount fill, a branch, or a
fallback composed over such a chain, bottoming at the per-mount top-level
fill. Chains the walk cannot reach keep the denial — the boundary stays
fail-closed, not gone.

## Trade-offs accepted

- The recursive walk emits per-mount statements only when the content
  actually carries nested outlets/events/refs — every existing unit's
  emission is byte-identical (D-8: the committed counter unit and all 37
  compile-clean gen modules re-verified unchanged).
- A unit whose per-mount fill nests several outlets pays one guarded walk
  chain per mount, the same cost class as the M18 fill mounts it rides
  beside.
- The `mut` receiver is threaded alongside the read receiver through the
  walk (the per-mount guard needs `as_ref`/`as_mut` spellings the build
  path's single receiver could not express).

## Consequences

- Refs inside nested slot content of a branch-mounted use compile, capture
  per mount, and re-capture across remount cycles — pinned by the two-leg
  `per-mount-nested-fill-ref` fixture, a codegen shape test, the demo
  suite, and the `per_mount_nested_fill_refs` walkthrough.
- The two silent fail-opens close: nested fills of branch-mounted uses
  mount (their binds go live), and the child's root-outlet fallback refs
  capture per mount.
- The budget ledger grows the `codegen_per_mount_nested_fill_ref` row (31
  rows).
- Carried gaps unchanged: cross-component branch-mount fill coordination
  (ADR-024 §4 — still a real design), branch-in-item and nested-list
  events (the E118 foundation follow-up), `lig-taffy-profile` parked,
  `cargo vet` deferred.
