# M8 milestone report — structural slot content (branch-in-content, list-in-content, composed `Ctx` contexts)

Date: 2026-11-06 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (the first row of M7's "known gaps", If/For half)

M7's report deferred "If/For/Component/Slot inside slot content":
branch/list tables would mount into the wrong fragment, and component
prop-pass crosses into a grandchild. M8 lifts the If/For half — fills and
fallbacks are now full program surfaces (binds, events, refs, AND If/For
at any depth) — through the composed `Ctx` contexts ADR-020 reserved the
base field for. Component-in-content and nested outlets in content stay
fail-closed (one precise reason each); they need grandchild prop-pass and
composed fill contexts, not branch walks. One addressing discipline, no
new crates, no new error codes, no new external dependencies, no IR change
(schema stays v5), **zero runtime changes** — the mounted content was
already a full `Fragment` with content-scoped branch/list tables; the gap
was codegen addressing.

| Exit criterion | Result |
| --- | --- |
| **Branch-in-content** | ✅ An If inside a fill/fallback mounts its branch ON the content (`slot_fill_mut`/`slot_fallback_mut` → the content receiver → `mount_branch`); the branch's own binds walk the composed chain (`slot_fill_mut` → `branch_mut` → the bind). The mount marks the fresh branch subtree's non-handler reactions — the flush re-enters and initializes them, so a branch bind whose reads are disjoint from its cond initializes on EVERY mount (the demo pins a late remount picking up a label written while the branch was absent). Branch events attach across slabs at mount; branch refs capture per mount through the read walk. |
| **List-in-content** | ✅ A keyed For inside a fill reconciles the content's OWN list table: the run arm pre-clones source + reads, unwraps the composed walk, and calls the receiver-param associated fn (`Self::reconcile_fN(h, recv, slot, items, …)` — no `self` borrow overlap); `plan_keyed` + the per-item bind replay run verbatim on the content's table. The demo rotates a keyed list inside the content. |
| **Composed `Ctx`** | ✅ `Branch { slot, base: Box<Ctx> }` composes over the parent's context — `ctx_of` recurses (branch wrapper → content map → `Direct`), `ctx_scope` unwraps outermost-first with shadowing receivers, `ctx_frag_read` yields `Option<&Fragment>` for mount-time reads. The composition also closes two M7-era latent holes the flat enum could not express: nested Ifs (an inner-branch bind addressed `self.frag.branch_mut` and skipped the outer level) and For-in-branch (the reconcile method hard-addressed `self.frag`). Both pinned by codegen contract tests. |
| **Subtree-wide fallback init-flush** | ✅ With structural fallbacks, the content's branch/list fragments own reactions — the parent's exact per-reaction mask covers the whole fallback SUBTREE (event handlers excluded by id): the fallback's If arms mount the content's own branches and its branch binds initialize inside the parent-driven flush. |
| **Fail-closed boundaries held** | ✅ Component uses and nested outlets inside content stay denied (`LIG-E001`, one precise reason each — grandchild prop-pass / composed fill contexts); events inside content For bodies stay denied (the LIG-E118 discipline). The Vue leg's content-ROOT directives move from silently-dropped to explicitly-shaped: `v-if`/`v-for` roots admit through `fragment_from_content_root` (the M7-era strip fail-open closes), while `v-else-if`/`v-else` roots (no chain to join), root component tags, and root `<slot>` tags deny — the last closing a second fail-open (`lower_element` does not dispatch the tag, so a nested outlet lowered as a plain element). |

## Fixture

✅ One new behavior fixture, both legs each (24 → 25):

| Fixture | Pins |
| --- | --- |
| `slots-structural` | Use 1 fills `header` with a `<div>` carrying an If (`<b class="hl">{label}</b>` under `on()`) and a keyed For (`<li>{t}</li>` keyed by the item); the `foot` outlet is unfilled, so the child's own structural fallback (`<div><Show when={title === "Status"}><i class="fb">{title}</i></Show></div>` / `v-if="title === 'Status'"`) mounts. One canonical IR: the fill fragment carries If + For dynamics with branch/body children, the fallback fragment carries an If. |

The KR-1 demo pins the runtime behavior on the mock host: the fill branch
mounted at build with its bind initialized, the keyed list on the
content's own table, the late remount re-initializing the branch bind
through the mount-time marking (label rewritten while unmounted), the
keyed reconcile inside the content, and the fallback branch initialized by
the parent-driven subtree-wide init-flush.

## Engineering baselines held

- **TDD**: 268 tests across 39 test binaries (M7: 263/39). The codegen
  landed with two contract tests (composed content addressing + nested
  If composition); both frontends with admit + deny contract tests; the
  fixture and its demo assertion pin the end-to-end behavior.
- **clippy pedantic**: `cargo clippy --workspace --all-targets -- -D
  warnings` green, benches included; no new allowlist rows.
- **Benches from day one**: `cargo bench --workspace --no-run` stays a CI
  gate; the M8 features ride the existing workloads (content branch/list
  arms are run arms; the fixture adds no new bench surface).
- **CI/CD**: unchanged job set — the new fixture rides the parity gate
  (25 fixtures, both legs), the new gen module rides compile-clean; no new
  gate, no new error code, the admission ladder is unchanged (22 cases,
  11 rules).
- **git-cliff**: changelog continues on conventional commits (`v0.1.0-m8`).
- **Zero new external dependencies**: the Cargo graph is untouched.

## Full gate run at delivery

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ |
| `cargo test --workspace` | ✅ 268 tests / 39 binaries |
| `cargo bench --workspace --no-run` | ✅ |
| `lig fixtures verify` | ✅ 25/25 fixtures, both legs |
| `lig fixtures admission` | ✅ 22/22 cases (NFR-5) |
| `lig fixtures rules-sync` | ✅ 11 rules 1:1 with fixtures |
| `lig reproducibility` | ✅ byte-identical (7 437 B counter unit — the counter carries no content-rooted structure; digest unchanged `0x85d06a5c087b3589`) |
| `lig incremental` | ✅ 6 passes green (D-9 + LIG-E020) |
| `lig errors-gen` | ✅ 15 codes + 15 embedded docs, regeneration is a no-op |
| `lig patch-spike` | ✅ two generations, **259 ms** total, < 1 s per save |
| `lig budgets` (release) | ✅ all 10 rows within budget — keyed row 3.9 µs / 25 µs (6.5×), codegen 577 µs / 3 000 µs, unit 7 437 B / 8 192 B |
| `lig no-std` | ✅ proof green (2 bare-metal targets + the generated unit) |
| `lig doctor` | ✅ all environment checks pass (view IR v5) |

## Notes and decisions

- **Why zero runtime changes**: `mount_slot` already instantiates the
  content as a full `Fragment` (slab, holes, branch tables, list tables,
  handlers, dirty set) from the authoring tree's statics — M6/M7's fill
  statics work carried the M8 shapes for free. The gap was purely that
  codegen had no way to ADDRESS a branch table behind `slot_fill_mut`;
  composing `Ctx` (codegen-only) closes it without touching the runtime.
- **Why the mount marks the branch's binds instead of re-running them
  inline**: the flush loop consumes the component's single dirty set; the
  mount arm marks the subtree's reaction ids and lets the loop re-enter
  (`produced.union(&rx.dirty().take())`) — the same convergence mechanism
  as memo writes, no special-cased replay. One redundant pass at build for
  initially-mounted branches (the bind runs in the init mask AND the mount
  mark) is invisible: binds are idempotent writes.
- **Why the composed For reconcile is an associated fn**: a `&mut self`
  method cannot take the receiver fragment (the walk's `&mut self.field`
  borrow would overlap `&mut self`), and cloning reads inside a method
  that also holds the walk borrow reintroduces the conflict. Passing the
  receiver + pre-cloned reads by value to a fn with no `self` keeps the
  borrow discipline mechanical. The Direct shape stays byte-stable.
- **Why `Fill`/`Fallback` keep no base field**: a component instance is
  always a struct field of the authoring component (`self.<field>.frag`),
  never reached through a tree walk — fills/fallbacks are walk ROOTS, only
  branches compose over them. A base field arrives (if ever) with
  composed fill contexts for nested outlets.
- **Why content-root `v-if`/`v-for` admit on the Vue leg**: the M7 denial
  existed because `fragment_from_elem` strips directives before lowering —
  a root `v-if` would silently vanish (the M6-era fail-open shape M7
  closed by denying). Lowering the structural root ONTO the content
  fragment (`fragment_from_content_root`) is the honest fix; the Solid leg
  already spelled roots via `<Show>` in fill position. Chain tails
  (`v-else-if`/`v-else`) have no siblings to join at a root and stay
  denied.
- **Why the fixture's branch bind reads a disjoint field**: `{label}`
  under `when={on()}` is exactly the case the mount marking exists for —
  the setter marks only `on`'s readers, so without the marking a remounted
  branch would render stale. The demo rewrites `label` while the branch is
  absent and asserts the remount picks it up.

## Known gaps (deferred, deliberate)

- Component uses inside slot content and nested outlets inside slot
  content: fail-closed (`LIG-E001`). Component-in-content needs grandchild
  prop-pass THROUGH mounted instances; nested outlets need composed fill
  contexts (a `Fill` base field and per-instance addressing).
- Direct-rooted branch binds initialize via co-read marking only (the M0
  shape is unchanged): a bind on the component's OWN branch whose reads
  are disjoint from the cond does not re-run on a late mount. The
  content-scoped mount marking is the M8 discipline; extending it to
  Direct-rooted branches is a one-line follow-up if a fixture ever needs
  it.
- The `else_`/`v-else` fragment lowers to the IR but the mount arm mounts
  the `then` branch only (pre-existing since M0 — the mount arm is a
  branch-slot toggle). Independent of content contexts; deferred.
- `v-model` on non-input elements (checkbox/radio/select) and
  `<slot :name>` forwarding: fail-closed today (M6 gap, unchanged).
- Template refs on the Vue leg: the Solid leg's `ref={local}` works end to
  end (the ref-capture split + the M8 mount-time captures pin it), but the
  Vue template has no `ref` spelling yet — a two-leg ref fixture waits for
  that lowering.
- `cargo vet` (§14.2 supply-chain onboarding, ADR-012): still deferred —
  the pin table remains the reviewed surface.
- A `lig patch` developer driver over real app dylibs (app-crate scaffold
  + dev channel): the engine surface is what that work wires into.
