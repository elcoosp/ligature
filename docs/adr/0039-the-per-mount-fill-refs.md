# ADR-0039: The per-mount fill refs (the ref-composed guard)

- Status: accepted
- Deciders: compiler team
- Date: 2026-11-26

## Context

The per-mount fill assembly (M18, ADR-0031) fixed the fills carried into
branch-mounted instances: the bind and event read/unwrap guards on the
`Option<Child>` field (`ctx_scope` / `ctx_frag_read`), and the mounts ride
the branch mounting arm's leg walk. The M18 report's gap list carried one
named follow-up:

- **Ref captures inside those fills.** The build-body emission
  (`content_ref_inits`) assumed a plain instance field — for a
  branch-mounted use the field is `Option<Child>`, so the pre-M18 spelling
  (`s.<field>.frag.slot_fill(..)`) refused to compile (fail-loud, the
  generated unit broken). A ref inside the fill of a branch-mounted use
  could not ship at all.

The timing constraint is the whole design: the instance is built in the
branch mounting arm, AFTER the build body runs — so the capture cannot
happen at build time even guarded; it must happen when the fill mounts.

## Decision

### 1. Per-mount fill refs capture at mount time

The `Ctx::Fill` ref-capture split: `per_mount == false` keeps the build
body emission byte-for-byte (direct uses, whose fills mount during the
build's slot assembly); `per_mount == true` routes the capture into the
mounting arm's leg walk, emitted after the per-outlet fill mount and event
attaches, guarded on the instance field:

```text
if let Some(inst) = self.<field>.as_ref() {
    if let Some(c) = inst.frag.slot_fill(<outlet>) {
        self.ref_<name> = c.hole(<hole>);
    }
}
```

The ownership rule is the M7 fill-ref discipline, per mount: the fill
content is authored at the use site, so the PARENT's `ref_<name>` field
takes the hole; fallback refs stay the child's fields (the child's own
trees handle them), unchanged. Because the mounting arm re-runs per mount,
the capture re-runs per mount — a remount cycle takes the fresh fill
element, never the disposed one (the `NodeId` discipline keeps the stale id
inert).

### 2. The refs pre-collect before the reaction loop

The If mounting arm is emitted inside the reaction loop, before the ref
reactions may have been visited — so the per-mount refs pre-collect into a
map keyed by the fill fragment (the leg walk meets that same fragment as
the outlet's content) before the loop starts. The leg walk emits nothing
when the map has no entry for the content: every unit without per-mount
fill refs is byte-identical (D-8 untouched; the committed counter digest
and all compile-clean gen modules re-verified unchanged).

### 3. Deeper nesting stays fail-closed

A ref inside NESTED slot content of a branch-mounted use (the ctx chain's
base beyond `Direct`) still denies with a precise codegen error — the leg
walk has no capture walk for it, and a silent sentinel would be the M6-era
fail-open shape. The guard boundary stays honest.

## Trade-offs accepted

- The capture is emitted once per (use site, outlet) into each leg's
  rebuild list — a unit whose use site carries several filled outlets pays
  one guarded read per outlet per mount, the same cost class as the M18
  fill mounts and event attaches it rides beside.
- The build-body skip means the ref field is written only when its branch
  mounts; a ref whose branch never mounted stays at the `NodeId(u32::MAX)`
  sentinel — the same never-mounted discipline branch refs have had since
  M8.

## Consequences

- The M18 gap list closes: refs inside fills carried into branch-mounted
  instances compile, capture per mount, and re-capture across remount
  cycles — pinned by the two-leg `per-mount-fill-ref` fixture, a codegen
  shape test, the demo suite, and the `per_mount_fill_refs` walkthrough.
- The budget ledger grows the `codegen_per_mount_fill_ref` row (30 rows).
- Carried gaps unchanged: cross-component branch-mount fill coordination
  (the ADR-024 §4 mechanism the branch legs' honesty prerequisite freed —
  still a real design), `lig-taffy-profile` parked, `cargo vet` deferred.
