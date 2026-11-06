//! Fragments, dynamics, anchors, and control flow (§5.3, §5.4).
//!
//! Component bodies, `if` branches, `for` bodies, and slot fallbacks are all
//! fragments (the Vapor boundary). `If`/`For`/components are fragment-valued
//! dynamics on the parent fragment.

use crate::anchor::Anchor;
use crate::expr::Ty;
use crate::{ComponentId, ExprId, FragmentId, StrId};
use serde::{Deserialize, Serialize};

/// A fragment: one template plus its dynamic entries and nested fragments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragmentIr {
    /// Stable, codegen-friendly symbol (collision-checked; Vapor's
    /// reserved-names discipline).
    pub name: StrId,
    /// The static shape this fragment instantiates.
    pub template: crate::TemplateId,
    /// One per hole/attr/event, in template order.
    pub dynamics: Vec<DynamicIr>,
    /// Nested branch/for/slot/component bodies.
    pub children: Vec<FragmentId>,
    /// What the parent passes in (§7.6).
    pub props: Vec<PropDecl>,
    /// Anchor refs the parent may address (Vapor `returns`).
    pub returns: Vec<Anchor>,
}

/// A dynamic entry: one hole, addressed by a precomputed anchor walk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DynamicIr {
    /// Static walk from the template root to the hole.
    pub anchor: Anchor,
    /// What kind of hole this is.
    pub kind: DynamicKind,
    /// The driving expression.
    pub expr: ExprId,
    /// `None` ⇒ build-time-only (static-by-construction).
    pub reaction: Option<crate::ReactionId>,
    /// Source span of the construct (attribute value / child expression) —
    /// admission diagnostics and the src→IR map (§9.4).
    pub span: (u32, u32),
}

/// Hole kinds (§5.3) — including the fragment-valued control-flow cases
/// (§5.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DynamicKind {
    /// Dynamic text child.
    Text,
    /// `innerHTML`-class content (tiered by host, §6.2).
    Html,
    /// Dynamic attribute.
    Attr {
        /// Attribute name.
        key: StrId,
    },
    /// Dynamic class.
    Class,
    /// Single dynamic style property.
    StyleProp {
        /// Property name.
        prop: StrId,
    },
    /// Whole style object.
    Style,
    /// Event listener; `delegate` honored per host capability (C7).
    Event {
        /// Event name from the fixed table.
        event: EventName,
        /// Whether delegation was requested.
        delegate: bool,
    },
    /// Template ref → `Ctx` field (no runtime registry).
    Ref,
    /// Children slot pass-through.
    Children,
    /// Pass a prop into a child fragment.
    PropPass {
        /// Child fragment receiving the prop.
        child: FragmentId,
        /// Index into the child's `props`.
        prop: u16,
    },
    /// Slot outlet; the parent's fill (or the outlet's own fallback)
    /// mounts into the hole at build time (§7.4).
    Slot {
        /// Slot name (`default` for the children outlet).
        name: StrId,
        /// Outlet-side fallback content, shown when a use site provides no
        /// fill for this name. A static subtree fragment (v1 slot-content
        /// contract) mounted by the parent's build.
        fallback: Option<FragmentId>,
    },
    /// Conditional — branch bodies are full fragments (C4).
    If(IfIr),
    /// List — keyed by `key` ⇒ LIS diff, else index diff (§7.5).
    For(ForIr),
    /// Component instantiation.
    Component(ComponentIr),
}

/// Conditional fragment (test memoized separately from branches — §2.1
/// `transformCondition`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IfIr {
    /// The test expression (its own reaction).
    pub cond: ExprId,
    /// Then-branch fragment.
    pub then: FragmentId,
    /// Optional else/else-if fragment (chains nest).
    pub else_: Option<FragmentId>,
}

/// List fragment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForIr {
    /// Source expression (the list).
    pub source: ExprId,
    /// Item binding name.
    pub item: StrId,
    /// Optional index binding.
    pub index: Option<StrId>,
    /// Key expression; `Some` ⇒ keyed LIS diff (§7.5).
    pub key: Option<ExprId>,
    /// Item view fragment.
    pub body: FragmentId,
}

/// Component instantiation (§5.4; `ComponentId` resolves through the module's
/// component table).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComponentIr {
    /// Index into `ViewModule::components`.
    pub component: ComponentId,
    /// Bound props.
    pub props: Vec<PropBinding>,
    /// Slot fills.
    pub slots: Vec<SlotIr>,
}

/// A prop binding at a component use site.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropBinding {
    /// Prop name.
    pub prop: StrId,
    /// Value expression.
    pub value: ExprId,
}

/// A slot fill at a component use site.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlotIr {
    /// Slot name.
    pub name: StrId,
    /// Fallback content fragment.
    pub fallback: Option<FragmentId>,
}

/// A component declaration: name, root fragment, prop list (§5.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComponentDecl {
    /// Component name (codegen symbol).
    pub name: StrId,
    /// Root fragment (a component root).
    pub root: FragmentId,
    /// Declared props.
    pub props: Vec<PropDecl>,
}

/// A prop declaration on a fragment or component.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropDecl {
    /// Prop name.
    pub name: StrId,
    /// Declared type.
    pub ty: Ty,
    /// Whether the parent may omit it.
    pub optional: bool,
}

/// The fixed event-name table (C7 — Solid's `DelegatedEvents` set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventName {
    /// beforeinput
    Beforeinput,
    /// click
    Click,
    /// dblclick
    Dblclick,
    /// contextmenu
    Contextmenu,
    /// focusin
    Focusin,
    /// focusout
    Focusout,
    /// input
    Input,
    /// keydown
    Keydown,
    /// keyup
    Keyup,
    /// mousedown
    Mousedown,
    /// mouseenter
    Mouseenter,
    /// mouseleave
    Mouseleave,
    /// mousemove
    Mousemove,
    /// mouseout
    Mouseout,
    /// mouseover
    Mouseover,
    /// mouseup
    Mouseup,
    /// pointerdown
    Pointerdown,
    /// pointermove
    Pointermove,
    /// pointerout
    Pointerout,
    /// pointerover
    Pointerover,
    /// pointerup
    Pointerup,
    /// touchcancel
    Touchcancel,
    /// touchend
    Touchend,
    /// touchmove
    Touchmove,
    /// touchstart
    Touchstart,
}

impl EventName {
    /// Parse a DOM event name from an attribute (`onClick`, `@input`,
    /// `v-on:keydown` — the `on` prefix is stripped by the callers).
    #[must_use]
    pub fn from_name(name: &str) -> Option<EventName> {
        Some(match name.to_ascii_lowercase().as_str() {
            "click" => EventName::Click,
            "input" => EventName::Input,
            "keydown" => EventName::Keydown,
            "keyup" => EventName::Keyup,
            "dblclick" => EventName::Dblclick,
            "contextmenu" => EventName::Contextmenu,
            "focusin" => EventName::Focusin,
            "focusout" => EventName::Focusout,
            "beforeinput" => EventName::Beforeinput,
            "mousedown" => EventName::Mousedown,
            "mouseenter" => EventName::Mouseenter,
            "mouseleave" => EventName::Mouseleave,
            "mousemove" => EventName::Mousemove,
            "mouseout" => EventName::Mouseout,
            "mouseover" => EventName::Mouseover,
            "mouseup" => EventName::Mouseup,
            "pointerdown" => EventName::Pointerdown,
            "pointermove" => EventName::Pointermove,
            "pointerout" => EventName::Pointerout,
            "pointerover" => EventName::Pointerover,
            "pointerup" => EventName::Pointerup,
            "touchcancel" => EventName::Touchcancel,
            "touchend" => EventName::Touchend,
            "touchmove" => EventName::Touchmove,
            "touchstart" => EventName::Touchstart,
            _ => return None,
        })
    }

    /// Every delegatable event name, in table order (the `DELEGATABLE` set).
    pub const DELEGATABLE: &'static [EventName] = &[
        EventName::Beforeinput,
        EventName::Click,
        EventName::Dblclick,
        EventName::Contextmenu,
        EventName::Focusin,
        EventName::Focusout,
        EventName::Input,
        EventName::Keydown,
        EventName::Keyup,
        EventName::Mousedown,
        EventName::Mouseenter,
        EventName::Mouseleave,
        EventName::Mousemove,
        EventName::Mouseout,
        EventName::Mouseover,
        EventName::Mouseup,
        EventName::Pointerdown,
        EventName::Pointermove,
        EventName::Pointerout,
        EventName::Pointerover,
        EventName::Pointerup,
        EventName::Touchcancel,
        EventName::Touchend,
        EventName::Touchmove,
        EventName::Touchstart,
    ];

    /// The kebab-case HTML name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            EventName::Beforeinput => "beforeinput",
            EventName::Click => "click",
            EventName::Dblclick => "dblclick",
            EventName::Contextmenu => "contextmenu",
            EventName::Focusin => "focusin",
            EventName::Focusout => "focusout",
            EventName::Input => "input",
            EventName::Keydown => "keydown",
            EventName::Keyup => "keyup",
            EventName::Mousedown => "mousedown",
            EventName::Mouseenter => "mouseenter",
            EventName::Mouseleave => "mouseleave",
            EventName::Mousemove => "mousemove",
            EventName::Mouseout => "mouseout",
            EventName::Mouseover => "mouseover",
            EventName::Mouseup => "mouseup",
            EventName::Pointerdown => "pointerdown",
            EventName::Pointermove => "pointermove",
            EventName::Pointerout => "pointerout",
            EventName::Pointerover => "pointerover",
            EventName::Pointerup => "pointerup",
            EventName::Touchcancel => "touchcancel",
            EventName::Touchend => "touchend",
            EventName::Touchmove => "touchmove",
            EventName::Touchstart => "touchstart",
        }
    }
}

/// A declared state field (signals, memos, props, locals — §5.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateDecl {
    /// Field name.
    pub name: StrId,
    /// Which fragment owns it (component roots own the component state).
    pub owner: FragmentId,
    /// Kind of state.
    pub kind: StateKind,
    /// Type.
    pub ty: Ty,
    /// Initial expression, if any.
    pub init: Option<ExprId>,
}

/// State kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StateKind {
    /// `createSignal` — a plain field + setter.
    Signal,
    /// `createMemo` — cached, equality-gated (§7.2).
    Memo,
    /// Component prop (§7.6).
    Prop,
    /// Non-reactive local.
    Local,
    /// Compiler-generated condition-test holder (§2.1 `transformCondition`).
    Cond,
}

/// Cross-module import (schema present at M0; resolution lands with M1
/// multi-module builds).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Import {
    /// Local name.
    pub name: StrId,
    /// Source module path.
    pub from: StrId,
    /// What kind of symbol.
    pub kind: ImportKind,
}

/// Import kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportKind {
    /// A component.
    Component,
    /// A pure function.
    Fn,
}

#[cfg(test)]
mod tests {
    use super::{EventName, IfIr};
    use crate::{ExprId, FragmentId};

    #[test]
    fn delegated_events_table_matches_the_fixed_set() {
        assert_eq!(EventName::DELEGATABLE.len(), 25);
        assert!(EventName::DELEGATABLE.contains(&EventName::Click));
        assert!(EventName::DELEGATABLE.contains(&EventName::Keydown));
    }

    #[test]
    fn event_names_are_kebab() {
        assert_eq!(EventName::Dblclick.as_str(), "dblclick");
        assert_eq!(EventName::Pointerup.as_str(), "pointerup");
    }

    #[test]
    fn if_ir_serializes() {
        let if_ir = IfIr {
            cond: ExprId::new(0),
            then: FragmentId::new(1),
            else_: None,
        };
        let json = serde_json::to_string(&if_ir).unwrap();
        assert!(json.contains("\"cond\":0"));
    }
}
