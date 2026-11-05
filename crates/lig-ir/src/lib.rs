//! View IR v2 — the only cross-framework contract (D-1, §5).
//!
//! A Block/Template/DynamicInfo model: deduped static templates + per-hole
//! dynamic entries with anchor paths + per-fragment reaction tables. The
//! parser AST stays in the oxc arena and is never serialized; this owned,
//! serde-based IR is the only serialized artifact (§2.5, §5.6).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod anchor;
pub mod canonical;
pub mod expr;
pub mod fragment;
pub mod module;
pub mod style;
pub mod template;
pub mod validate;

pub use anchor::{Anchor, AnchorStep, NodeRef};
pub use canonical::{canonicalize_helpers, normalize_frontend_json};
pub use expr::{BinOp, Expr, ExprTable, Lit, TplPart, Ty, UnOp};
pub use fragment::{
    ComponentDecl, ComponentIr, DynamicIr, DynamicKind, EventName, ForIr, FragmentIr, IfIr, Import,
    ImportKind, PropBinding, PropDecl, SlotIr, StateDecl, StateKind,
};
pub use module::{
    ReactionDecl, ReactionKind, SourceFile, SpanEntry, SpanTable, Tier, VIEW_IR_VERSION, ViewModule,
};
pub use style::{StyleConst, StyleLonghand, StyleValue};
pub use template::{AttrValue, NodeIr, StaticAttr, TemplateIr};

// Ids are re-exported from lig-core (§4.2: ids live in lig-core).
pub use lig_core::id::{
    ComponentId, ExprId, FileId, FragmentId, PureFnId, ReactionId, SpanId, StrId, StyleConstId,
    StylePropId, TemplateId,
};

/// Struct shape id for the `Ty::Struct` case (§5.5 lattice).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct ShapeId(pub u32);

impl ShapeId {
    /// Raw index.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Frontend header field — the only field allowed to differ in parity
/// fixtures (§6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Frontend {
    /// Solid TSX input.
    Solid,
    /// Vue SFC input.
    Vue,
    /// Canonical/expected artifacts are frontend-agnostic.
    Expected,
}
