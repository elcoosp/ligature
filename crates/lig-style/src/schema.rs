//! The profile v1 property schema: exactly 25 properties, shipped as pure
//! data (§8.2 — the dom-expressions table discipline). `StylePropId` is the
//! index into this table; order is canonical.

/// One schema row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropSchema {
    /// CSS property name.
    pub css_name: &'static str,
    /// How values are interpreted and expanded.
    pub kind: ValueKind,
    /// The taffy composite field this maps to (profile v1), or `None` for
    /// keyword-only fields like `display`.
    pub taffy_field: TaffyField,
}

/// Value interpretation kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    /// A single value (length/percent/auto/keyword/number).
    One,
    /// A box shorthand expanded to t/r/b/l (margin, padding).
    Rect,
    /// A space-separated list (gap: `8px 16px` → row col).
    List,
}

/// The taffy composite field a property feeds (profile v1 mapping, §8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaffyField {
    /// `display` (keyword enum).
    Display,
    /// `position` (keyword enum).
    Position,
    /// Inset rect side.
    Inset {
        /// Which box side.
        side: Side,
    },
    /// Size dimension side (`Top` = width, `Bottom` = height).
    Size {
        /// Which axis.
        side: Side,
    },
    /// Min-size side (`Top` = width, `Bottom` = height).
    MinSize {
        /// Which axis.
        side: Side,
    },
    /// Max-size side (`Top` = width, `Bottom` = height).
    MaxSize {
        /// Which axis.
        side: Side,
    },
    /// Aspect ratio (number).
    AspectRatio,
    /// Margin rect side.
    Margin {
        /// Which box side.
        side: Side,
    },
    /// Padding rect side.
    Padding {
        /// Which box side.
        side: Side,
    },
    /// Flex-direction keyword.
    FlexDirection,
    /// Flex-wrap keyword.
    FlexWrap,
    /// Flex-grow number.
    FlexGrow,
    /// Flex-shrink number.
    FlexShrink,
    /// Flex-basis dimension.
    FlexBasis,
    /// Gap size (row/col).
    Gap {
        /// Which axis.
        axis: Axis,
    },
    /// Align-items keyword.
    AlignItems,
    /// Align-self keyword.
    AlignSelf,
    /// Justify-content keyword.
    JustifyContent,
    /// Align-content keyword.
    AlignContent,
    /// Not layout-mapped (host extras).
    None,
}

/// Box sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// top
    Top,
    /// right
    Right,
    /// bottom
    Bottom,
    /// left
    Left,
}

/// Rect axes for gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// row (main).
    Row,
    /// column (cross).
    Column,
}

/// The 25-property profile v1 table. Order is the canonical property order.
pub const PROFILE_V1: &[PropSchema] = &[
    PropSchema {
        css_name: "display",
        kind: ValueKind::One,
        taffy_field: TaffyField::Display,
    },
    PropSchema {
        css_name: "position",
        kind: ValueKind::One,
        taffy_field: TaffyField::Position,
    },
    PropSchema {
        css_name: "top",
        kind: ValueKind::One,
        taffy_field: TaffyField::Inset { side: Side::Top },
    },
    PropSchema {
        css_name: "right",
        kind: ValueKind::One,
        taffy_field: TaffyField::Inset { side: Side::Right },
    },
    PropSchema {
        css_name: "bottom",
        kind: ValueKind::One,
        taffy_field: TaffyField::Inset { side: Side::Bottom },
    },
    PropSchema {
        css_name: "left",
        kind: ValueKind::One,
        taffy_field: TaffyField::Inset { side: Side::Left },
    },
    PropSchema {
        css_name: "width",
        kind: ValueKind::One,
        taffy_field: TaffyField::Size { side: Side::Top },
    },
    PropSchema {
        css_name: "height",
        kind: ValueKind::One,
        taffy_field: TaffyField::Size { side: Side::Bottom },
    },
    PropSchema {
        css_name: "min-width",
        kind: ValueKind::One,
        taffy_field: TaffyField::MinSize { side: Side::Top },
    },
    PropSchema {
        css_name: "min-height",
        kind: ValueKind::One,
        taffy_field: TaffyField::MinSize { side: Side::Bottom },
    },
    PropSchema {
        css_name: "max-width",
        kind: ValueKind::One,
        taffy_field: TaffyField::MaxSize { side: Side::Top },
    },
    PropSchema {
        css_name: "max-height",
        kind: ValueKind::One,
        taffy_field: TaffyField::MaxSize { side: Side::Bottom },
    },
    PropSchema {
        css_name: "aspect-ratio",
        kind: ValueKind::One,
        taffy_field: TaffyField::AspectRatio,
    },
    PropSchema {
        css_name: "margin",
        kind: ValueKind::Rect,
        taffy_field: TaffyField::Margin { side: Side::Top },
    },
    PropSchema {
        css_name: "padding",
        kind: ValueKind::Rect,
        taffy_field: TaffyField::Padding { side: Side::Top },
    },
    PropSchema {
        css_name: "flex-direction",
        kind: ValueKind::One,
        taffy_field: TaffyField::FlexDirection,
    },
    PropSchema {
        css_name: "flex-wrap",
        kind: ValueKind::One,
        taffy_field: TaffyField::FlexWrap,
    },
    PropSchema {
        css_name: "flex-grow",
        kind: ValueKind::One,
        taffy_field: TaffyField::FlexGrow,
    },
    PropSchema {
        css_name: "flex-shrink",
        kind: ValueKind::One,
        taffy_field: TaffyField::FlexShrink,
    },
    PropSchema {
        css_name: "flex-basis",
        kind: ValueKind::One,
        taffy_field: TaffyField::FlexBasis,
    },
    PropSchema {
        css_name: "gap",
        kind: ValueKind::List,
        taffy_field: TaffyField::Gap { axis: Axis::Row },
    },
    PropSchema {
        css_name: "align-items",
        kind: ValueKind::One,
        taffy_field: TaffyField::AlignItems,
    },
    PropSchema {
        css_name: "align-self",
        kind: ValueKind::One,
        taffy_field: TaffyField::AlignSelf,
    },
    PropSchema {
        css_name: "justify-content",
        kind: ValueKind::One,
        taffy_field: TaffyField::JustifyContent,
    },
    PropSchema {
        css_name: "align-content",
        kind: ValueKind::One,
        taffy_field: TaffyField::AlignContent,
    },
];

/// Look up a property by CSS name.
#[must_use]
pub fn lookup(css_name: &str) -> Option<(lig_core::id::StylePropId, &'static PropSchema)> {
    PROFILE_V1
        .iter()
        .position(|p| p.css_name == css_name)
        .map(|i| (lig_core::id::StylePropId::new(i as u32), &PROFILE_V1[i]))
}

#[cfg(test)]
mod tests {
    use super::{PROFILE_V1, lookup};

    #[test]
    fn exactly_25_properties() {
        assert_eq!(PROFILE_V1.len(), 25, "profile v1 is the 25-property table");
    }

    #[test]
    fn names_unique_and_ordered() {
        let mut seen = std::collections::BTreeSet::new();
        for (i, p) in PROFILE_V1.iter().enumerate() {
            assert!(seen.insert(p.css_name), "duplicate {}", p.css_name);
            assert_eq!(lookup(p.css_name).unwrap().0.get(), i as u32);
        }
    }

    #[test]
    fn core_properties_present() {
        assert!(lookup("display").is_some());
        assert!(lookup("gap").is_some());
        assert!(lookup("align-content").is_some());
        assert!(lookup("float").is_none(), "float is not layout-mappable");
        assert!(lookup("color").is_none(), "colors are host extras in v1");
    }
}
