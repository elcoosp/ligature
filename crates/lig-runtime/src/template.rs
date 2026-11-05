//! Const template trees (C1/C2): the "clone a `<template>`" trick of the web
//! frameworks becomes "iterate a const tree" — fully const-constructible via
//! static slices, zero parsing at instantiate time (§10.1).

use crate::host::{AttrVal, StyleConstId};

/// A static attribute in a template node.
#[derive(Debug, Clone, Copy)]
pub struct TplAttr {
    /// Attribute name.
    pub key: &'static str,
    /// Static value.
    pub val: AttrVal<'static>,
}

/// One node of the static shape.
#[derive(Debug, Clone, Copy)]
pub enum TplNode {
    /// An element with static attributes and children.
    Element {
        /// Tag name.
        tag: &'static str,
        /// Static attributes (inlined into the template, §5.2).
        attrs: &'static [TplAttr],
        /// Interned static style const.
        style: Option<StyleConstId>,
        /// Child nodes.
        children: &'static [TplNode],
    },
    /// Static text.
    Text {
        /// Text content.
        value: &'static str,
    },
    /// Dynamic child hole — the `<!>` marker (C3).
    Marker,
}

/// A template: const slice of root nodes.
pub type Template = &'static [TplNode];

/// One addressing step of an anchor walk (§5.3): `Down(i)` enters the i-th
/// child, `Next` moves to the next sibling, `MarkerDown(i)` enters the i-th
/// child and requires it to be a marker hole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorStep {
    /// Enter the i-th child.
    Down(u32),
    /// Move to the next sibling.
    Next,
    /// Enter the i-th child; it must be a marker.
    MarkerDown(u32),
}
