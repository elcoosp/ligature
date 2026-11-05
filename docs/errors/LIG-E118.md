# LIG-E118 — Event handler inside a For body

Status: deny (M1)

## message
Event handler inside a For body

## explanation
Per-item dispatch needs keyed component instances (M2). The v1 subset fails closed rather than mis-routing events (§7.5).

## suggestion
Hoist the handler to the list level, or extract the row into a component.
