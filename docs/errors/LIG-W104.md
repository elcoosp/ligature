# LIG-W104 — Inline Rust escape hatch

Status: lint (M1)

## message
Inline Rust escape hatch

## explanation
native("…") lowers to raw Rust tokens — it is opaque to the optimizer and outside the static dataflow (§6.5)

## suggestion
 you own it, the compiler does not.:Keep snippets small and pure; move logic into memos where possible.
