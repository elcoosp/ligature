# ADR-0025: M12 — template refs on the Vue leg

- Status: accepted (M12)
- Deciders: compiler team
- Date: 2026-11-09

## Context

The two-frontend contract is one canonical View IR: "the same component
spelled twice compiles to byte-identical IR." The `Ref` dynamic has been
part of that contract since M2 and works end to end on the Solid leg
(`ref={box}` over a bare `let box;` — the generated unit declares a
sentinel-initialized `ref_box: NodeId` field and captures the element's
hole in the build body). The Vue leg had NO template spelling for it:

- `ref="box"` lowered as a STATIC ATTRIBUTE — a literal `ref="box"` in the
  rendered HTML, silently disconnected from the script (the fail-open).
- `:ref="box"` lowered as `DynamicKind::Attr { key: "ref" }` — a dynamic
  attribute write, never a node capture (the same fail-open in a dynamic
  shape).
- The placeholder declaration itself, `let box;` (init-less), was denied
  with `LIG-E001` ("declarations without initializers are outside the
  admitted subset") — so even a correct `ref` attribute had no legal way
  to name its target. The Solid leg's bare `let box;` lowers to nothing
  (the ref capture owns the name).

M10's known-gaps list carried this as "template refs on the Vue leg: the
Solid leg's `ref={local}` works end to end; the Vue template has no `ref`
spelling yet." The deep dive's contribution was the fail-open framing: the
gap was not merely a missing feature — the two `ref` shapes silently
mis-compiled as attributes, the exact admission-ladder violation class
M9/M10 closed elsewhere (dropped directives, detached roots).

(ADR numbering note: 0024 is reserved by the in-flight M11 branch — the
`else` leg — which predates this document in the remote.)

## Decision

### 1. The capture contract is admission's job

A template ref binds a plain placeholder local — a `Local` read, the exact
shape the codegen `Ref` arm already admits (`ref must be a local binding in
v1`). Admission owns every other shape's diagnosis, with precise codes
instead of downstream failures:

- **Bare `ref`** → `LIG-E001` (no target).
- **Static `ref="x"` naming a signal** → `LIG-E001` (a capture would
  shadow the setter write path; two-way binding is the `v-model`
  discipline).
- **Static `ref="x"` naming nothing** → `LIG-E100` (the same unknown
  identifier the bound form's lowering produces — one code, both
  spellings).
- **Bound `:ref="expr"` over a non-`Local`** (members, calls, binary
  shapes, signal reads) → `LIG-E001` ("template refs bind a placeholder
  local").

### 2. `let x;` is the Vue spelling of the Solid placeholder

Init-less declarators over plain identifiers are admitted as **placeholder
locals**: recorded into the lowering's non-reactive local set (the set the
`v-for` item/index bindings live in), creating NO state and NO reactions.
The expression lowering already resolves locals to `Expr::Local`, so the
bound form rides the existing machinery; the static form interns the name
and inserts the same `Expr::Local`. Both spellings produce the identical
`DynamicKind::Ref` dynamic with empty `reads` — byte-identical IR across
legs, which the `template-ref` fixture pins.

Destructuring patterns stay denied (the pre-M12 `LIG-E001` text stands for
them) — a placeholder is a single name.

### 3. Zero codegen and runtime changes

The `Ref` codegen arm, the `ref_<name>` field emission, the sentinel
`NodeId(u32::MAX)` init, and the per-ctx capture (`Ctx::Direct` in the
build body, content/branch/fallback captures at mount time) are all
pre-existing M2–M8 machinery. The Vue leg now reaches it through one new
lowering path (`lower_template_ref`) that both spellings converge on.
Elements that dispatch elsewhere keep their behavior: component tags pass
`ref` through as a prop name exactly as the Solid leg does (parity), and
`<slot>` outlets keep their directive discipline.

## Consequences

- The admission surface is symmetric: a labeled-input component compiles
  on both legs with byte-identical IR (fixture `template-ref`, both legs,
  `lig fixtures verify` green).
- The silent mis-lowerings are gone: `ref="box"` can no longer render as a
  literal attribute, and `:ref="box"` can no longer become an `Attr`
  dynamic — both spellings either capture or deny.
- Component instance refs (`<Badge ref="b">`) remain the prop-pass-through
  both legs share; instance-handle refs are a different story (per-use
  identity rides M10's `Option<Child>` fields) and stay out of scope.
- Per-item refs inside list bodies follow the Solid leg's existing shape
  (`Expr::Local` of the item binding) — no new contract, no new denials.
- The bench ledger grows the `codegen_ref` row (M12 reference median
  234 µs against a 1 200 µs ceiling, ≈5.1× headroom).
