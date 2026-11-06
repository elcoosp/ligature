# ADR-018: M5 — keyed `For` (§7.5) end to end

- Status: accepted (M5)
- Deciders: compiler team
- Date: 2026-11-06

## Context

§7.5's list contract has two halves. Unkeyed lists (`.map` children, plain
`<For>`/`v-for`) take the Index diff — shipped at M0/M2. Keyed lists take a
`key` expression and reconcile through an LIS-stable diff, and every
milestone report from M2 on carried the same deferral note: *"the schema
exists (`ForIr.key`), both frontends emit index-diff lists."* The runtime
even shipped the full substrate early — `keyed_diff`, `lis_sequence`, and
the slab `Fragment` list ops (`list_insert` with keys, `list_move`,
`list_key`) were benched and budget-pinned since M0/M4 — while nothing in
the compiler ever produced a key. M5 closes the gap end to end: both
frontends, codegen, a two-leg fixture, and the runtime primitive the
generated code runs.

## Decision

### 1. Surface syntax: idiomatic on each leg, one canonical shape

- **Solid**: `<For each={tags()} key={(t) => t}>` — a ligature extension in
  the Solid attribute position (real Solid's `<For>` keys by reference; the
  explicit `key` spells the identity this compiler diffs on). The arrow
  takes exactly the body's item binding; anything else is fail-closed
  (`LIG-E001`).
- **Vue**: `<li v-for="t in tags" :key="t">` — Vue's own spelling,
  longhand `v-bind:key` included. The attribute is *stripped* from the item
  fragment: the key drives diffing, it is not an attribute of the item
  template (and the Solid leg's `li` carries none — the two legs must lower
  to one canonical IR).
- Both lower to `ForIr.key: Option<ExprId>` — a `Local` read of the item
  (identity) or any admitted pure projection of it. A static `key="…"`
  gives every item the same identity and is denied (`LIG-E001`).

### 2. Keys are identities: pure over the item, or nothing

The key expression may read the item and the index accessor and nothing
else. Component-state reads are denied at admission (`LIG-E001`, both
frontends) *and* re-checked at codegen (`CodegenError::Unsupported`):
a state-dependent key re-identifies items on unrelated writes — every
setter flush would churn the list. Keys must be unique within a list
(documented contract; duplicates make the old→live mapping ambiguous).

### 3. The u64 identity space, normalized at codegen

The runtime diff runs on `u64` keys. Codegen emits a per-For `key_of_fN`
fn: string keys hash through `lig_runtime::key_hash` (FNV-1a 64,
dependency-free, host-deterministic — `lig-runtime` keeps its zero-dep
no_std posture, so the hash lives there, not in `lig-core`); numeric and
bool keys cast. List-typed keys are fail-closed. A generated unit for the
keyed fixture is committed (`tests/compile-clean/src/gen/for_keyed.rs`)
and the KR-1 demo exercises it on the mock host.

### 4. Execution: a sequential plan, not a raw op list

`keyed_diff`'s op list is *intent*: removes and moves address the old
sequence, so applying it directly against the live slab corrupts positions
(the exhaustive permutation harness fails on `[1,2,3,4] → [2,4,3,1]`).
The shipped executor is `plan_keyed(old, new) -> Vec<SeqOp>`: removes
first (multiset-aware), then a left-to-right prefix walk — position `k`
settles by one step (a move found right of the prefix, or an insert), the
prefix invariant guarantees the walk lands exactly on `new`. Generated
reconcile methods stay a plain sequential `match` over `SeqOp` mapping
onto the existing slab ops, followed by the same full bind replay the
unkeyed path runs (index-driven binds must track the new positions after
moves). The plan trades the diff's move-minimality for index-free
sequential execution: move counts stay within n−1 and match the LIS bound
for reversals and front insertions; rotation-class updates may pay one
extra move. The M4 `runtime_list_diff_keyed` budget row now measures the
planner (the shipped path), not the diff.

### 5. The permutation pins caught a latent diff bug

Building the exhaustive old×new harness surfaced a real M0-era bug:
`keyed_diff` anchored its LIS over the surviving keys' *values* instead of
the surviving old *indices* (Vue's `getSequence` criterion). Value-order
and index-order disagree exactly on swaps — `[2, 1] → [1, 2]` anchored
both items and emitted zero moves. Fixed to LIS over indices, pinned by a
dedicated test; the ledger's diff workload moves to the planner.

### 6. What M5 deliberately does not do

- No new crates, no new external dependencies, no IR schema change
  (`ForIr.key` was already v2); the fixture set grows by one (`for-keyed`,
  both legs), the rule table is untouched (keyed `For` admits, it does not
  diagnose).
- `.map` children stay unkeyed (a `key` option on `.map` is future work);
  nested control flow inside item bodies remains denied (unchanged v1
  subset).
- `cargo vet`, named slots, `v-model`, multi-statement effect bodies, and
  the `lig patch` developer driver remain deferred (M4 known gaps, minus
  the keyed-`For` line this milestone closes).

## Consequences

- Keyed lists now reconcile the way §7.5 describes: a reorder moves item
  fragments instead of destroying and recreating them (KR-1 demo: a
  3-item rotation is exactly one host `Move`, zero `Create`/`Destroy`).
- The `fixtures verify` count rises to 19; the ledger row
  `runtime_list_diff_keyed` measures `plan_keyed` (3.8 µs vs the 25 µs
  ceiling at delivery — 6.6× headroom).
- `plan_keyed`/`SeqOp`/`key_hash` join `lig-runtime`'s public, no_std,
  zero-dependency contract surface.
