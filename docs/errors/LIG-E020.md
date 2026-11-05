# LIG-E020 — Cyclic component import

Status: reserved (M3 — incremental pipeline)

## message
Cyclic component import: `{cycle}`

## explanation
Import cycles among components cannot be assigned a fragment build order
(§11.1). Salsa cycle recovery surfaces this as a Deny diagnostic, never a
hang.

## suggestion
Break the cycle by extracting the shared subtree into a third component.
