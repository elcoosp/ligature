//! Interned static styles (D-12, §8): profile-normalized longhand lists,
//! content-addressed and deduped module-wide.

use crate::ShapeId;
use lig_core::id::{StrId, StylePropId};
use serde::{Deserialize, Serialize};

/// One interned static style: ordered normalized longhands plus the canonical
/// CSS text (for host passthrough bags).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyleConst {
    /// 64-bit content hash over the canonical longhand encoding.
    pub hash: u64,
    /// Canonical CSS declaration text, e.g. `display:flex;padding:16px`.
    pub css: String,
    /// Ordered `(property, value)` longhands.
    pub longhands: Vec<StyleLonghand>,
}

/// A single normalized longhand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyleLonghand {
    /// Property-schema index (§8.1 table).
    pub prop: StylePropId,
    /// Normalized value.
    pub value: StyleValue,
}

/// Normalized style values (the subset the 25-property profile v1 needs).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum StyleValue {
    /// Absolute length in px.
    Len(f32),
    /// Percentage 0-100.
    Pct(f32),
    /// `auto`.
    Auto,
    /// Keyword (interned, e.g. `flex`, `wrap`).
    Keyword(StrId),
    /// Color as string (interned; hosts resolve).
    Color(StrId),
    /// Unitless number.
    Num(f32),
    /// Unrecognized-but-kept value (never silently dropped, §8.2).
    Raw(StrId),
}

impl StyleValue {
    /// Canonical serialization token (used by style hashing).
    #[must_use]
    pub fn token(&self) -> String {
        match self {
            StyleValue::Len(l) => format!("len:{l}"),
            StyleValue::Pct(p) => format!("pct:{p}"),
            StyleValue::Auto => "auto".to_string(),
            StyleValue::Keyword(k) => format!("kw:{k}"),
            StyleValue::Color(c) => format!("color:{c}"),
            StyleValue::Num(n) => format!("num:{n}"),
            StyleValue::Raw(r) => format!("raw:{r}"),
        }
    }
}

/// A structural record shape (referenced by `Ty::Struct`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapeField {
    /// Field name.
    pub name: StrId,
    /// Field type.
    pub ty: String,
}

/// Registered shapes (module-level; referenced by id).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ShapeTable {
    shapes: Vec<Vec<ShapeField>>,
}

impl ShapeTable {
    /// Register a shape, returning its id.
    pub fn insert(&mut self, fields: Vec<ShapeField>) -> ShapeId {
        let id = ShapeId(self.shapes.len().try_into().expect("shape table overflow"));
        self.shapes.push(fields);
        id
    }

    /// Look up a shape's fields.
    #[must_use]
    pub fn get(&self, id: ShapeId) -> Option<&[ShapeField]> {
        self.shapes.get(id.get() as usize).map(Vec::as_slice)
    }

    /// Number of shapes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.shapes.len()
    }

    /// Whether there are no shapes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{StyleConst, StyleLonghand, StyleValue};
    use lig_core::id::StrId;

    #[test]
    fn value_tokens_are_distinct() {
        let a = StyleValue::Len(16.0).token();
        let b = StyleValue::Pct(16.0).token();
        let c = StyleValue::Num(16.0).token();
        assert_ne!(a, b);
        assert_ne!(b, c);
    }

    #[test]
    fn style_const_serializes() {
        let s = StyleConst {
            hash: 42,
            css: "display:flex".to_string(),
            longhands: vec![StyleLonghand {
                prop: lig_core::id::StylePropId::new(0),
                value: StyleValue::Keyword(StrId::new(1)),
            }],
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"css\":\"display:flex\""));
    }
}
