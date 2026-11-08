# ADR-023: M10 — per-mount component identity and the branch-time slot assembly

- Status: accepted (M10)
- Deciders: compiler team
- Date: 2026-11-08

## Context

M9's known gaps (ADR-022 consequences): component uses and outlets inside
BRANCH/LIST bodies of slot content fail-closed with `LIG-E001` (a remounted
branch would re-insert a root the previous teardown destroyed — instances
need per-mount identity, per-item identity the keyed-component story), and
outlets inside the child's OWN branch fragments never mounted anything (the
deep dive confirmed the shape compiles and renders an empty hole — a
silent fail-open, not a denial). The deep dive also exposed two further
silent fail-opens in the same mount-time family: a DIRECT component use's
root never entered its owner's tree (the commit stream had Create commits
but no positioning `Host::insert` — every nested component tree rendered
detached since M2), and the Vue frontend dropped `v-if`/`v-for` on
component and slot TAGS entirely (the dispatch checked the tag before the
directive, so `<Badge v-if="on">` lowered as an unconditional use — the
same fail-open shape the M9 content-root dispatch closed).

## Decision

### 1. The universal attach contract (zero runtime changes)

Every component use's root enters its owner's tree through `Host::insert`
exactly once per mount, with identity matching the mount lifetime:

- **Direct uses** — the build body inserts the child's root at the use hole
  (`h.insert(s.frag.hole(di), s.<field>.frag.roots()[0], 0)`) before slot
  assembly; the initial flush then writes props onto a tree-attached child.
- **Content uses** — the M9 story, unchanged: the parent's slot assembly
  inserts the grandchild at content mount.
- **Branch uses** — per-mount identity (below): the mounting arm rebuilds
  the instance and inserts its root into the branch's component hole.

The commit-stream primitive is the existing `Host::insert`; the runtime is
untouched, and the counter component digest is unchanged
(`0x85d06a5c087b3589` — the unit bytes grew only by the build-phase doc).

### 2. Per-mount instance identity

A component use on an If-BRANCH fragment (any base — direct-rooted or
content-rooted, author side or child side) owns an `Option<Child>` field:

- **Mount** — the If arm rebuilds the instance
  (`self.<f> = Some(Child::build(h, Props::default()))`) and inserts its
  root into the branch's component hole through the composed read walk.
- **Dispose** — the unmount arm takes the field and tears the instance's
  slab down (`if let Some(inst) = self.<f>.take() { inst.frag.teardown(h); }`)
  — a remount cycle never reuses a stale instance or leaks its slab.
- **Prop-pass** — the arm guards on the instance
  (`if let Some(c) = self.<f>.as_mut()`); the exact build mask covers the
  mount marking (the reaction is owned by the branch fragment), so writes
  made while the branch was absent land on the NEXT mount — the M8
  late-mount re-initialization discipline generalized to instances.

For-body (list item) uses stay fail-closed with one precise reason
(per-item identity is the keyed-component story) at both the frontends and
a codegen backstop.

### 3. The branch-time slot assembly

An outlet on a branch fragment mounts its content WITH the branch: the
mounting If arm, after `mount_branch`, walks the branch through the
composed accessor chain and mounts the use-site fill (`SlotOrigin::Fill`)
or the outlet's own fallback (`SlotOrigin::Fallback`), then recurses into
the freshly mounted content for its outlets and component holes (the M9
walk, now rooted at a mounted branch). The mount mask widens by the
branch-mounted fills' reactions (author-side fills are not under the
branch subtree), so the fresh content's binds initialize on the next
flush pass; the origin gates hold unchanged.

- **Author-side content branches** (the If hangs on a fill/fallback the
  use site owns): fills matched by name from the SAME use site,
  `build_content_ctx` extended with a branch leg (`map_branch_outlets`)
  whose fill contexts compose `Fill { base: Branch { .. } }`.
- **Child-side branches** (the If hangs on the child's own tree): the
  outlet's own fallback mounts child-side — closing the fail-open where
  the hole rendered empty forever. FILLS for child-side branch outlets
  stay codegen-denied: mounting parent-authored content from the child's
  mount arm needs a cross-component coordination mechanism (the follow-up
  milestone's story); the M9 unmatched-fill backstop keeps them honest.

### 4. The Vue structural-directive dispatch (fail-open closed)

`v-if`/`v-for` now dispatch BEFORE the component/slot tag dispatch, and
`fragment_from_elem` dispatches component/slot BRANCH roots (the
generalization of M9's `fragment_from_content_root`), so `<Badge v-if>`,
`<slot v-for>`, and their `v-else*` companions lower onto the same
If/For + branch-fragment shapes the Solid leg produces. Two legs, one
canonical IR: the new fixtures converge without parity-ignored fields.

### 5. The lint ladder is attribute-free

The gate now greps the tree for zero `#[allow]`/`#![allow]` attributes:
the workspace table carries `pedantic = warn` (CI's `-D warnings`
escalates), every suppression lives in `[lints]` tables with a rationale
comment, `lig-patch`'s unsafe surface and the generated-code hygiene live
in per-crate tables or `#![expect]` with reasons, and the generator emits
plain code — the generated lint blanket is gone (the compile-clean crate's
`[lints]` own the suppression, reviewed). Note: a command-line
`-W clippy::pedantic` re-enables pedantic-group members past any
`[lints]` allow (rustc's last-flag-wins), so the gate that respects the
documented overrides is `cargo clippy --all-targets --all-features -- -D
warnings` with pedantic carried by the workspace table.

## Consequences

- Fills, fallbacks, and component uses compose over EVERY mount shape the
  runtime has; the slot-content contract is: dynamic (M7) → structural
  (M8) → component/nested (M9) → per-mount identity + branch-time
  assembly (M10).
- Three verified silent fail-opens closed: detached direct-use roots,
  never-mounted child-branch outlet fallbacks, and the Vue-leg dropped
  structural directives on component/slot tags.
- The remaining content gaps: fills for child-side branch-internal
  outlets (branch-mount-time fill coordination across components), 
  per-item instance identity (the keyed-component story), `v-model` on
  non-input elements, `<slot :name>` forwarding, and template refs on the
  Vue leg.
- The counter unit is byte-stable in shape (digest unchanged); the new
  shapes ride the `codegen/branch` bench workload with a ledger row
  (`codegen_branch`, §14.2).
