# LIG-E401 — Reactive cycle at runtime

Status: active

## message
LIG-E401 reactive cycle at runtime

## explanation
The const topological schedule (§7.2) converged in more than `MAX_PASSES`
passes, which means reactions keep re-dirtying each other. The static cycle
check should have denied this at compile time; this assertion is the
fail-safe.

## suggestion
Inspect the component's reaction table (`lig inspect`) for a reaction that
writes a state field it (transitively) reads.
