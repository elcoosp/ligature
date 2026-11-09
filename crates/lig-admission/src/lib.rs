//! Admission (D-6, §6.5): the Deny / Lint / Allow ladder over a lowered View
//! IR module. Admission is a **query over the IR** — every construct lands in
//! exactly one tier; the rule table (`docs/admission/rules.md`) is the source
//! of truth and each row links its allow/deny fixtures (V.6/V.7 policy).
//!
//! Fail-closed (BR-1): any Deny-tier diagnostic means zero output downstream.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod rules;

use lig_core::diagnostic::{LigDiagnostic, Severity};
use lig_ir::ViewModule;
use lig_ir::expr::Expr;
use lig_ir::fragment::{DynamicKind, FragmentIr, StateKind};

/// The host style profile (§8.2): tiering context for host-dependent
/// constructs (`v-html` is a Lint on web-capable hosts, a Deny on
/// layout-only profiles).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profile {
    /// Layout-only profile (taffy): no DOM primitives.
    #[default]
    Layout,
    /// Web-capable host: rich commit stream (html, focus, clipboard).
    Web,
}

/// The result of admitting one module.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdmissionReport {
    /// Deny diagnostics (severity Error) and Lint diagnostics (Warning).
    pub diagnostics: Vec<LigDiagnostic>,
    /// Constructs inside the compiled subset (Allow tier) — reported for
    /// `lig check` summaries.
    pub allowed: usize,
}

impl AdmissionReport {
    /// Whether the module is admitted (no Deny diagnostics — BR-1).
    #[must_use]
    pub fn is_admitted(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Deny diagnostics.
    pub fn denies(&self) -> impl Iterator<Item = &LigDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
    }

    /// Lint diagnostics.
    pub fn lints(&self) -> impl Iterator<Item = &LigDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
    }

    /// `--deny=warn` (§12.1): promote every Lint to a Deny.
    #[must_use]
    pub fn with_lints_denied(mut self) -> Self {
        for d in &mut self.diagnostics {
            if d.severity == Severity::Warning {
                d.severity = Severity::Error;
            }
        }
        self
    }
}

/// Admit one lowered module under `profile`.
#[must_use]
pub fn admit(module: &ViewModule, profile: Profile) -> AdmissionReport {
    let mut report = AdmissionReport::default();
    // 1. Lowering-time Lints (spread W101, style objects W102) re-rendered
    //    with their spans — they carry concrete source ranges.
    for note in &module.lints {
        report
            .diagnostics
            .push(lint(&note.code, note.message.clone(), note.start, note.end));
    }
    // 2. Expression-forest lints: `native("…")` is the documented escape
    //    hatch — lowered, Lint-tagged, opaque to the optimizer (§6.5).
    for (idx, expr) in module.exprs.as_slice().iter().enumerate() {
        if let Expr::Native(_) = expr {
            let sp = module.exprs.span_of(lig_core::id::ExprId::new(idx as u32));
            report.diagnostics.push(lint(
                "LIG-W104",
                "native(…) is inline Rust — you own it, the compiler does not (§6.5)".to_string(),
                sp.start,
                sp.end,
            ));
        }
    }
    // 3. Per-fragment walks.
    let owned_bodies = owned_body_fragments(module);
    for (idx, frag) in module.fragments.iter().enumerate() {
        admit_fragment(
            module,
            lig_core::id::FragmentId::new(idx as u32),
            frag,
            profile,
            &mut report,
            &owned_bodies,
        );
    }
    // 4. Deep prop chains (§6.5 Lint): prop reads through ≥2 field hops in
    //    any lowered expression (first hit wins; spans are per-expression).
    for (idx, expr) in module.exprs.as_slice().iter().enumerate() {
        if chain_depth_on_props(module, expr) >= 2 {
            let sp = module.exprs.span_of(lig_core::id::ExprId::new(idx as u32));
            report.diagnostics.push(lint(
                "LIG-W103",
                "deep prop chain in a view expression — flatten it in a memo for cheaper updates"
                    .to_string(),
                sp.start,
                sp.end,
            ));
            break; // one lint per module is enough to point at the pattern
        }
    }
    report.allowed = count_allowed(module);
    report
}

fn admit_fragment(
    module: &ViewModule,
    frag_id: lig_core::id::FragmentId,
    frag: &FragmentIr,
    profile: Profile,
    report: &mut AdmissionReport,
    owned_bodies: &std::collections::HashSet<u32>,
) {
    for d in &frag.dynamics {
        let (start, end) = d.span;
        match &d.kind {
            DynamicKind::Html => {
                // v-html class constructs: tiered by host profile (§6.2).
                match profile {
                    Profile::Web => report.diagnostics.push(lint(
                        "LIG-W105",
                        "raw HTML content — sandbox the source before it reaches user data"
                            .to_string(),
                        start,
                        end,
                    )),
                    Profile::Layout => {
                        let mut diag = LigDiagnostic::error(
                            "LIG-E117",
                            "raw HTML is denied on layout-only profiles (no DOM primitive to render it)",
                            lig_core::span::Span::new(start, end),
                        );
                        diag.help = Some(
                            "render text via {expr}, or use the web profile for HTML-capable hosts"
                                .into(),
                        );
                        report.diagnostics.push(diag);
                    }
                }
            }
            DynamicKind::For(for_ir) => {
                // LIG-E118, M15: an event handler DIRECTLY on a For body
                // admits — per-item dispatch rides the keyed instance
                // machinery (M13's per-item stores, the per-item replay,
                // index-addressed fire fns). The body's SUBTREE keeps the
                // fail-closed discipline: events inside list-item branches,
                // nested lists, and COMPOSED lists (branch- or slot-owned —
                // their dispatch addressing is the follow-up) deny with one
                // precise reason at the event's own span. The v1 check was
                // flat (the body's direct dynamics only), so nested events
                // slipped through admission to the codegen backstop; the
                // walk closes that hole.
                let owner_direct = !owned_bodies.contains(&frag_id.get());
                let mut seen = std::collections::HashSet::new();
                walk_item_body_events(module, for_ir.body, owner_direct, report, &mut seen);
            }
            _ => {}
        }
    }
}

/// Fragments owned as a branch/list/slot body by some dynamic (the
/// `branch_owner_of` codegen rule, mirrored): a For whose OWNER fragment is
/// in this set hangs on a branch or inside another list — a composed list,
/// whose dispatch addressing is not the `&mut self` shape.
fn owned_body_fragments(module: &ViewModule) -> std::collections::HashSet<u32> {
    let mut owned = std::collections::HashSet::new();
    for f in &module.fragments {
        for d in &f.dynamics {
            match &d.kind {
                DynamicKind::If(i) => {
                    owned.insert(i.then.get());
                    if let Some(el) = i.else_ {
                        owned.insert(el.get());
                    }
                }
                DynamicKind::For(fo) => {
                    owned.insert(fo.body.get());
                }
                DynamicKind::Slot {
                    fallback: Some(fb), ..
                } => {
                    owned.insert(fb.get());
                }
                _ => {}
            }
        }
    }
    owned
}

/// The M15 item-body event walk: events on a DIRECT For body admit
/// (`owner_direct`), events anywhere deeper in the item subtree deny with
/// `LIG-E118` at their own span. `seen` dedups shared sub-fragments.
fn walk_item_body_events(
    module: &ViewModule,
    frag_id: lig_core::id::FragmentId,
    owner_direct: bool,
    report: &mut AdmissionReport,
    seen: &mut std::collections::HashSet<u32>,
) {
    if !seen.insert(frag_id.get()) {
        return;
    }
    let Some(frag) = module.fragments.get(frag_id.get() as usize) else {
        return;
    };
    for d in &frag.dynamics {
        match &d.kind {
            DynamicKind::Event { .. } => {
                if !owner_direct {
                    let (start, end) = d.span;
                    let mut diag = LigDiagnostic::error(
                        "LIG-E118",
                        "event handlers beyond the direct list item body are not admitted in v1",
                        lig_core::span::Span::new(start, end),
                    );
                    diag.help = Some(
                        "hoist the handler to the list level, or extract a keyed component".into(),
                    );
                    report.diagnostics.push(diag);
                }
            }
            DynamicKind::If(i) => {
                walk_item_body_events(module, i.then, false, report, seen);
                if let Some(el) = i.else_ {
                    walk_item_body_events(module, el, false, report, seen);
                }
            }
            DynamicKind::For(fo) => {
                // A nested list: its owner is a For body (never a direct-ctx
                // fragment), so the inner body's events stay denied.
                walk_item_body_events(module, fo.body, false, report, seen);
            }
            DynamicKind::Slot {
                fallback: Some(fb), ..
            } => {
                walk_item_body_events(module, *fb, false, report, seen);
            }
            _ => {}
        }
    }
    for &c in &frag.children {
        walk_item_body_events(module, c, false, report, seen);
    }
}

/// Number of `Field` hops above a Prop-kind state read (§6.5 W103).
fn chain_depth_on_props(module: &ViewModule, expr: &Expr) -> usize {
    match expr {
        Expr::Field(inner, _) => {
            let base = chain_depth_on_props(module, inner);
            if base > 0 || matches!(**inner, Expr::State(s) if is_prop(module, s)) {
                base + 1
            } else {
                0
            }
        }
        Expr::State(s) if is_prop(module, *s) => 0,
        Expr::Unary(_, inner) => chain_depth_on_props(module, inner),
        Expr::Binary(_, a, b) | Expr::Index(a, b) => {
            chain_depth_on_props(module, a).max(chain_depth_on_props(module, b))
        }
        Expr::Ternary(c, a, b) => chain_depth_on_props(module, c)
            .max(chain_depth_on_props(module, a))
            .max(chain_depth_on_props(module, b)),
        _ => 0,
    }
}

fn is_prop(module: &ViewModule, s: lig_core::id::StrId) -> bool {
    module
        .state_by_name(s)
        .is_some_and(|d| d.kind == StateKind::Prop)
}

fn count_allowed(module: &ViewModule) -> usize {
    // Allow-tier constructs: every dynamic that is not a deny case, every
    // non-native expression.
    let mut n = 0usize;
    for f in &module.fragments {
        for d in &f.dynamics {
            if !matches!(d.kind, DynamicKind::Html) {
                n += 1;
            }
        }
    }
    for e in module.exprs.as_slice() {
        if !matches!(e, Expr::Native(_)) {
            n += 1;
        }
    }
    n
}

fn lint(code: &str, message: String, start: u32, end: u32) -> LigDiagnostic {
    LigDiagnostic::warning(code, message, lig_core::span::Span::new(start, end))
}

#[cfg(test)]
mod tests {
    use super::{AdmissionReport, Profile, admit};
    use lig_core::diagnostic::Severity;
    use lig_ir::ViewModule;

    fn lower(src: &str) -> ViewModule {
        lig_parse_solid::lower_module(src, "fixture.tsx").expect("lowers")
    }

    fn lower_err(src: &str) -> Vec<lig_core::diagnostic::LigDiagnostic> {
        lig_parse_solid::lower_module(src, "fixture.tsx").expect_err("denied at lowering")
    }

    #[test]
    fn clean_module_is_admitted_with_allow_counts() {
        let m = lower(
            "export function A() {\n  const [n, setN] = createSignal(1);\n  return <p>{n()}</p>;\n}\n",
        );
        let r = admit(&m, Profile::Layout);
        assert!(r.is_admitted(), "{:?}", r.diagnostics);
        assert!(r.lints().next().is_none());
        assert!(r.allowed > 0);
    }

    #[test]
    fn dynamic_style_object_is_a_w102_lint_not_a_deny() {
        let m = lower(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/conformance/dynamic-style/input.solid.tsx"
        )));
        let r = admit(&m, Profile::Layout);
        assert!(r.is_admitted(), "lint must not deny: {:?}", r.diagnostics);
        assert!(
            r.lints().any(|d| d.code == "LIG-W102"),
            "{:?}",
            r.diagnostics
        );
    }

    #[test]
    fn deny_lints_promotion_turns_warnings_into_errors() {
        let m = lower(
            "export function A() {\n  const [c, setC] = createSignal(\"red\");\n  return <div style={{ color: c() }}>x</div>;\n}\n",
        );
        let promoted = admit(&m, Profile::Layout).with_lints_denied();
        assert!(!promoted.is_admitted(), "--deny=warn promotes lints");
        assert!(
            promoted
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error)
        );
    }

    #[test]
    fn html_is_tiered_by_profile() {
        let src = "export function A() {\n  const [h, setH] = createSignal(\"<b>x</b>\");\n  return <div innerHTML={h()} />;\n}\n";
        let m = lower(src);
        let web = admit(&m, Profile::Web);
        assert!(web.is_admitted());
        assert!(web.lints().any(|d| d.code == "LIG-W105"), "{web:?}");
        let layout = admit(&m, Profile::Layout);
        assert!(!layout.is_admitted());
        assert!(layout.denies().any(|d| d.code == "LIG-E117"));
    }

    #[test]
    fn for_handler_direct_item_body_admits() {
        // M15: an event handler DIRECTLY on a For body admits — per-item
        // dispatch rides the keyed instance machinery (M13's stores + the
        // per-item replay); the v1 denial's premise is gone.
        let src = "import { For } from \"solid-js\";\nexport function A() {\n  const [items, setItems] = createSignal([1, 2]);\n  return (\n    <ul>\n      <For each={items()}>{(n) => <li onClick={() => setItems([n])}>{n}</li>}</For>\n    </ul>\n  );\n}\n";
        let m = lower(src);
        let r = admit(&m, Profile::Layout);
        assert!(r.is_admitted(), "{r:?}");
        assert!(r.denies().next().is_none(), "{r:?}");
    }

    #[test]
    fn for_handler_in_item_branch_is_e118() {
        // M15 boundary: events nested deeper inside the item subtree — a
        // branch-in-item here — keep the fail-closed denial (per-item ×
        // per-mount composition is the follow-up). The walk must CATCH the
        // nested event (the v1 flat check only looked at the body's direct
        // dynamics and let this shape slip through to codegen).
        let src = "import { For, Show } from \"solid-js\";\nexport function A() {\n  const [items, setItems] = createSignal([1, 2]);\n  return (\n    <For each={items()}>\n      {(n) => (\n        <Show when={n > 0}>\n          <li onClick={() => setItems([n])}>{n}</li>\n        </Show>\n      )}\n    </For>\n  );\n}\n";
        let m = lower(src);
        let r = admit(&m, Profile::Layout);
        assert!(!r.is_admitted(), "{r:?}");
        let e118: Vec<_> = r.denies().filter(|d| d.code == "LIG-E118").collect();
        assert_eq!(e118.len(), 1, "one precise reason: {r:?}");
    }

    #[test]
    fn for_handler_in_nested_list_is_e118() {
        // M15 boundary: a For inside a For body is control-flow-in-item —
        // its item events keep the denial (nested-list dispatch is the
        // follow-up).
        let src = "import { For } from \"solid-js\";\nexport function A() {\n  const [groups, setGroups] = createSignal([[1, 2]]);\n  return (\n    <For each={groups()}>\n      {(g) => (\n        <For each={g}>\n          {(n) => <li onClick={() => setGroups([[n]])}>{n}</li>}\n        </For>\n      )}\n    </For>\n  );\n}\n";
        let m = lower(src);
        let r = admit(&m, Profile::Layout);
        assert!(!r.is_admitted(), "{r:?}");
        assert!(r.denies().any(|d| d.code == "LIG-E118"), "{r:?}");
    }

    #[test]
    fn deep_prop_chains_are_w103() {
        let src = "export function Card({ user }) {\n  return <p>{user.profile.name}</p>;\n}\n";
        let m = lower(src);
        let r = admit(&m, Profile::Layout);
        assert!(r.is_admitted(), "chain is lowered: {r:?}");
        assert!(r.lints().any(|d| d.code == "LIG-W103"), "{r:?}");
    }

    #[test]
    fn shallow_prop_reads_stay_silent() {
        let src = "export function Card({ name }) {\n  return <p>{name}</p>;\n}\n";
        let m = lower(src);
        let r = admit(&m, Profile::Layout);
        assert!(r.is_admitted());
        assert!(r.lints().next().is_none(), "{r:?}");
    }

    #[test]
    fn native_is_a_w104_lint() {
        let src = "export function A() {\n  const [n, setN] = createSignal(1);\n  return <p>{native(\"n\")}</p>;\n}\n";
        let m = lower(src);
        let r = admit(&m, Profile::Layout);
        assert!(r.is_admitted());
        assert!(r.lints().any(|d| d.code == "LIG-W104"), "{r:?}");
    }

    #[test]
    fn lowering_denies_arrive_as_errors() {
        let diags =
            lower_err("export function A() {\n  return <p>{items.filter(i => i)}</p>;\n}\n");
        assert!(diags.iter().any(|d| d.code == "LIG-E114"));
        assert!(diags.iter().any(|d| d.severity == Severity::Error));
    }

    #[test]
    fn report_default_is_empty_and_admitted() {
        let r = AdmissionReport::default();
        assert!(r.is_admitted());
        assert_eq!(r.allowed, 0);
        let _ = Profile::default();
    }
}
