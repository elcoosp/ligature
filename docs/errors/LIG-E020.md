# LIG-E020 — Cyclic component import

Status: active (M3 — incremental pipeline)

## message
Cyclic component import: `{cycle}`

## explanation
Import cycles among components cannot be assigned a fragment build order
(§11.1): the running app would recurse forever at first mount (component A
builds B builds A …). The incremental engine's build-order query detects the
cycle before codegen and surfaces it as this Deny diagnostic — the
salsa-style cycle recovery: a bounded diagnostic instead of a hang or a stack
overflow. `{cycle}` names the members in walk order, e.g. `A → B → A`; the
code frame labels the first use site on the cycle.

## suggestion
Break the cycle by extracting the shared subtree into a third component.
