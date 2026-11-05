//! The object-safe `Host` commit stream (D-5, §10.1): no associated types, no
//! generics — every method takes concrete or `Copy` types so the trait is
//! `dyn`-compatible across dylib boundaries.

use core::fmt;

/// A node handle: `Copy`, slab-allocated, globally unique per process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

impl NodeId {
    /// Raw index.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "n{}", self.0)
    }
}

/// A branch slot handle (fragment-valued dynamics: `if`/`for`/slots).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BranchId(pub u32);

impl BranchId {
    /// Raw index.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// An event handler slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HandlerId(pub u32);

impl HandlerId {
    /// Raw index.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A host-side lifecycle hook slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HookId(pub u32);

impl HookId {
    /// Raw index.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// An interned static style (D-12): hosts pull the record from the table they
/// bound once via their own `bind_style_table` convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StyleConstId(pub u32);

impl StyleConstId {
    /// Raw index.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// The event payload slice visible to generated handlers (§6.1: event
/// handlers compile to model functions with `&mut Ctx, Ev` signatures).
///
/// Only the admitted event fields are carried; hosts fill what they have and
/// generated code reads fields through `Option`. Owned `String`s keep the
/// runtime `no_std + alloc` and the struct `Clone` for handler capture.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvData {
    /// `e.currentTarget.value`, when the host provides it.
    pub current_target_value: Option<alloc::string::String>,
    /// `e.target.value`, when the host provides it.
    pub target_value: Option<alloc::string::String>,
    /// `e.key`, when the host provides it.
    pub key: Option<alloc::string::String>,
}

impl EvData {
    /// An empty payload (handlers that read it see `None`).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            current_target_value: None,
            target_value: None,
            key: None,
        }
    }

    /// Set `currentTarget.value`.
    #[must_use]
    pub fn current_target_value(mut self, v: impl Into<alloc::string::String>) -> Self {
        self.current_target_value = Some(v.into());
        self
    }

    /// Set `target.value`.
    #[must_use]
    pub fn target_value(mut self, v: impl Into<alloc::string::String>) -> Self {
        self.target_value = Some(v.into());
        self
    }

    /// Set `key`.
    #[must_use]
    pub fn key(mut self, v: impl Into<alloc::string::String>) -> Self {
        self.key = Some(v.into());
        self
    }
}

/// Static attribute values (pure data, §5.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AttrVal<'a> {
    /// String value.
    Str(&'a str),
    /// Boolean value.
    Bool(bool),
    /// Numeric value.
    Num(f64),
}

/// Dynamic style property values (`set_style_dyn`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PropVal<'a> {
    /// Length in px.
    Len(f32),
    /// Keyword.
    Keyword(&'a str),
    /// Color or other opaque host-owned token.
    Raw(&'a str),
}

/// The fixed event-kind table (C7 — Solid's `DelegatedEvents`, §6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
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

impl EventKind {
    /// Every delegatable kind, in table order.
    pub const DELEGATABLE: &'static [EventKind] = &[
        EventKind::Beforeinput,
        EventKind::Click,
        EventKind::Dblclick,
        EventKind::Contextmenu,
        EventKind::Focusin,
        EventKind::Focusout,
        EventKind::Input,
        EventKind::Keydown,
        EventKind::Keyup,
        EventKind::Mousedown,
        EventKind::Mouseenter,
        EventKind::Mouseleave,
        EventKind::Mousemove,
        EventKind::Mouseout,
        EventKind::Mouseover,
        EventKind::Mouseup,
        EventKind::Pointerdown,
        EventKind::Pointermove,
        EventKind::Pointerout,
        EventKind::Pointerover,
        EventKind::Pointerup,
        EventKind::Touchcancel,
        EventKind::Touchend,
        EventKind::Touchmove,
        EventKind::Touchstart,
    ];

    /// The kebab-case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            EventKind::Beforeinput => "beforeinput",
            EventKind::Click => "click",
            EventKind::Dblclick => "dblclick",
            EventKind::Contextmenu => "contextmenu",
            EventKind::Focusin => "focusin",
            EventKind::Focusout => "focusout",
            EventKind::Input => "input",
            EventKind::Keydown => "keydown",
            EventKind::Keyup => "keyup",
            EventKind::Mousedown => "mousedown",
            EventKind::Mouseenter => "mouseenter",
            EventKind::Mouseleave => "mouseleave",
            EventKind::Mousemove => "mousemove",
            EventKind::Mouseout => "mouseout",
            EventKind::Mouseover => "mouseover",
            EventKind::Mouseup => "mouseup",
            EventKind::Pointerdown => "pointerdown",
            EventKind::Pointermove => "pointermove",
            EventKind::Pointerout => "pointerout",
            EventKind::Pointerover => "pointerover",
            EventKind::Pointerup => "pointerup",
            EventKind::Touchcancel => "touchcancel",
            EventKind::Touchend => "touchend",
            EventKind::Touchmove => "touchmove",
            EventKind::Touchstart => "touchstart",
        }
    }
}

/// What `Host::create` receives: the static shape of one node (§10.1).
#[derive(Debug, Clone, Copy)]
pub struct TemplateDesc<'a> {
    /// Tag name for elements.
    pub tag: &'a str,
    /// Static attribute table.
    pub attrs: &'a [(&'a str, AttrVal<'a>)],
    /// Interned static style, if any.
    pub style: Option<StyleConstId>,
}

/// The host commit stream — hosts mirror state; they are never called
/// mid-build except through these commits (§10.2).
pub trait Host {
    /// Materialize a node (create → destroy happens in LIFO slab order).
    fn create(&mut self, node: NodeId, desc: TemplateDesc<'_>);
    /// Destroy a node.
    fn destroy(&mut self, node: NodeId);
    /// Replace text content.
    fn set_text(&mut self, node: NodeId, text: &str);
    /// Set or clear an attribute.
    fn set_attr(&mut self, node: NodeId, key: &str, val: Option<AttrVal<'_>>);
    /// Set or clear the class attribute.
    fn set_class(&mut self, node: NodeId, class: Option<&str>);
    /// Apply an interned static style const.
    fn set_style(&mut self, node: NodeId, style: StyleConstId);
    /// Apply dynamic style properties (prev-diffed by generated code, C5).
    fn set_style_dyn(&mut self, node: NodeId, props: &[(u32, PropVal<'_>)]);
    /// Replace raw HTML content (tiered: `Lint` on web-capable hosts, `Deny`
    /// on layout-only profiles, §6.2/§8.2). Hosts that cannot honor it must
    /// treat this as an error path — fail-closed mirrors the compiler.
    fn set_html(&mut self, node: NodeId, html: &str);
    /// Insert a child.
    fn insert(&mut self, parent: NodeId, child: NodeId, index: u32);
    /// Remove a child.
    fn remove(&mut self, parent: NodeId, child: NodeId);
    /// Move a child.
    fn move_(&mut self, parent: NodeId, child: NodeId, index: u32);
    /// Attach an event handler (delegation is a host capability flag, C7).
    fn attach(&mut self, node: NodeId, ev: EventKind, handler: HandlerId, delegate: bool);
    /// Detach an event handler.
    fn detach(&mut self, node: NodeId, ev: EventKind, handler: HandlerId);
    /// Host-side mount lifecycle.
    fn on_mount(&mut self, node: NodeId, hook: HookId);
    /// Host-side unmount lifecycle.
    fn on_unmount(&mut self, node: NodeId, hook: HookId);
}
