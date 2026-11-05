# LIG-E100 — Unknown identifier

Status: active

## message
Unknown identifier `{name}` in view position

## explanation
Every identifier in a view expression must resolve to a declared signal,
memo, prop, local, or an imported component through the scope analysis
(§7.3). Unresolved reads would make the static dataflow unsound, so they
are denied.

## suggestion
Declare the value in the component body (`createSignal`, `createMemo`, a
local `const`), or pass it as a prop. If it is arbitrary logic, move it
into a `model!{}` block (§6.5 escape hatches).
