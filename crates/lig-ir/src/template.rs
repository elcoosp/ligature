//! Templates and static shape (C1, §5.2): the static HTML shape extracted
//! into a reusable, content-addressed tree. Static attributes live on the
//! template node — there is no `SetStaticStyle` op in v2; a static style *is*
//! the template.

use crate::StyleConstId;
use lig_core::hash::fnv1a64;
use lig_core::id::StrId;
use serde::{Deserialize, Serialize};

/// A deduped, content-addressed static template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateIr {
    /// Root node of the static shape.
    pub root: NodeIr,
    /// 64-bit content hash over the canonical node encoding.
    pub hash: u64,
}

impl TemplateIr {
    /// Build a template, computing its content hash.
    #[must_use]
    pub fn new(root: NodeIr) -> Self {
        let hash = Self::compute_hash(&root);
        Self { root, hash }
    }

    /// Recompute the content hash (also the invariant checker's oracle).
    #[must_use]
    pub fn compute_hash(root: &NodeIr) -> u64 {
        fnv1a64(&encode_node(root))
    }
}

/// A static attribute on a template node — a pure data table entry (§6.1
/// `attrs.rs::DECISIONS` resolves its runtime strategy).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StaticAttr {
    /// Attribute name (interned, e.g. `class`, `title`).
    pub key: StrId,
    /// Static value.
    pub value: AttrValue,
}

/// Static attribute values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AttrValue {
    /// String value.
    Str(StrId),
    /// Boolean value (HTML boolean attributes).
    Bool(bool),
    /// Numeric value.
    Num(f64),
}

/// A node in the static template tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeIr {
    /// An element with its static attributes and children.
    Element {
        /// Tag name (interned).
        tag: StrId,
        /// Static attribute table.
        static_attrs: Vec<StaticAttr>,
        /// Interned static style, if this element carries one (§8).
        style: Option<StyleConstId>,
        /// Child nodes.
        children: Vec<NodeIr>,
    },
    /// Static text.
    Text {
        /// Text content (interned).
        value: StrId,
    },
    /// Dynamic child hole — the `<!>` placeholder of Solid, Vapor, and
    /// Svelte 5 (C3).
    Marker,
}

/// Encode one node into the canonical byte stream.
fn encode_walk(node: &NodeIr, out: &mut Vec<u8>) {
    match node {
        NodeIr::Element {
            tag,
            static_attrs,
            style,
            children,
        } => {
            out.push(b'E');
            out.extend_from_slice(tag.get().to_le_bytes().as_slice());
            out.extend_from_slice(
                style
                    .map_or(0u32, StyleConstId::get)
                    .to_le_bytes()
                    .as_slice(),
            );
            out.extend_from_slice(
                u32::try_from(static_attrs.len())
                    .unwrap_or(u32::MAX)
                    .to_le_bytes()
                    .as_slice(),
            );
            for a in static_attrs {
                out.extend_from_slice(a.key.get().to_le_bytes().as_slice());
                match &a.value {
                    AttrValue::Str(s) => {
                        out.push(b'S');
                        out.extend_from_slice(s.get().to_le_bytes().as_slice());
                    }
                    AttrValue::Bool(b) => {
                        out.push(b'B');
                        out.push(u8::from(*b));
                    }
                    AttrValue::Num(n) => {
                        out.push(b'N');
                        out.extend_from_slice(&n.to_le_bytes());
                    }
                }
            }
            out.extend_from_slice(
                u32::try_from(children.len())
                    .unwrap_or(u32::MAX)
                    .to_le_bytes()
                    .as_slice(),
            );
            for c in children {
                encode_walk(c, out);
            }
        }
        NodeIr::Text { value } => {
            out.push(b'T');
            out.extend_from_slice(value.get().to_le_bytes().as_slice());
        }
        NodeIr::Marker => out.push(b'M'),
    }
}

/// Encode a node tree canonically for hashing (structural, order-stable).
fn encode_node(node: &NodeIr) -> Vec<u8> {
    let mut out = Vec::new();
    encode_walk(node, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::{AttrValue, NodeIr, StaticAttr, TemplateIr};
    use lig_core::intern::StringTable;

    fn el(tag: &str, strings: &mut StringTable, children: Vec<NodeIr>) -> NodeIr {
        NodeIr::Element {
            tag: strings.intern(tag),
            static_attrs: Vec::new(),
            style: None,
            children,
        }
    }

    #[test]
    fn hash_is_content_addressed() {
        let mut strings = StringTable::new();
        let a = TemplateIr::new(el("div", &mut strings, vec![NodeIr::Marker]));
        let b = TemplateIr::new(el("div", &mut strings, vec![NodeIr::Marker]));
        assert_eq!(a.hash, b.hash, "identical trees share a hash");
        let c = TemplateIr::new(el("span", &mut strings, vec![NodeIr::Marker]));
        assert_ne!(a.hash, c.hash);
    }

    #[test]
    fn attr_values_affect_hash() {
        let mut strings = StringTable::new();
        let tag = strings.intern("div");
        let class = strings.intern("class");
        let v1 = strings.intern("a");
        let v2 = strings.intern("b");
        let mk = |v| {
            TemplateIr::compute_hash(&NodeIr::Element {
                tag,
                static_attrs: vec![StaticAttr {
                    key: class,
                    value: v,
                }],
                style: None,
                children: vec![],
            })
        };
        assert_ne!(mk(AttrValue::Str(v1)), mk(AttrValue::Str(v2)));
        assert_ne!(mk(AttrValue::Bool(true)), mk(AttrValue::Bool(false)));
    }

    #[test]
    fn marker_text_element_distinguishable() {
        let mut strings = StringTable::new();
        let marker = TemplateIr::compute_hash(&NodeIr::Marker);
        let text = TemplateIr::compute_hash(&NodeIr::Text {
            value: strings.intern("x"),
        });
        assert_ne!(marker, text);
    }
}
