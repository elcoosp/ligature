//! Profile v1: StyleIR → const-constructible `taffy::Style` (§8.2).
//!
//! taffy 0.15's `Style` carries a drop type, so direct struct-update in a
//! `const` is rejected by rustc; the const-constructible idiom taffy itself
//! ships (`Style::DEFAULT` + field assignment inside a `const fn`) is what
//! this emitter produces. The mapping table lives in `lig-style::schema`;
//! the taffy dependency is owned here (feature-gated per the workspace
//! layout, so hosts that don't use taffy never link it).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use lig_style::schema::TaffyField;
use lig_style::{StyleIr, StyleValue};

/// Emission result: a `const fn` returning the lowered style, plus anything
/// the profile could not map (tiered upstream — never silently dropped).
#[derive(Debug, Clone, PartialEq)]
pub struct LoweredStyle {
    /// `const fn <name>() -> taffy::Style { .. }` source text.
    pub const_fn_text: String,
    /// The fn name, for referencing from generated code.
    pub fn_name: String,
    /// Values the profile could not map (host extras).
    pub unmapped: Vec<String>,
}

fn fnum(v: f32) -> String {
    let s = format!("{v}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

/// taffy percentages are `[0.0, 1.0]` (dimension.rs); CSS percentages are
/// `[0, 100]` — the conversion happens here, once.
fn pct01(p: f32) -> f32 {
    p / 100.0
}

fn lpa(v: &StyleValue) -> Option<String> {
    // LengthPercentageAuto (const-fn constructors, taffy 0.15)
    Some(match v {
        StyleValue::Len(l) => format!("taffy::LengthPercentageAuto::length({})", fnum(*l)),
        StyleValue::Pct(p) => format!("taffy::LengthPercentageAuto::percent({})", fnum(pct01(*p))),
        StyleValue::Auto => "taffy::LengthPercentageAuto::auto()".to_string(),
        _ => return None,
    })
}

fn lp(v: &StyleValue) -> Option<String> {
    // LengthPercentage — no `auto` in this type
    Some(match v {
        StyleValue::Len(l) => format!("taffy::LengthPercentage::length({})", fnum(*l)),
        StyleValue::Pct(p) => format!("taffy::LengthPercentage::percent({})", fnum(pct01(*p))),
        _ => return None,
    })
}

fn dimension(v: &StyleValue) -> Option<String> {
    Some(match v {
        StyleValue::Len(l) => format!("taffy::Dimension::length({})", fnum(*l)),
        StyleValue::Pct(p) => format!("taffy::Dimension::percent({})", fnum(pct01(*p))),
        StyleValue::Auto => "taffy::Dimension::auto()".to_string(),
        _ => return None,
    })
}

fn keyword(v: &StyleValue) -> Option<&'static str> {
    match v {
        StyleValue::Keyword(k) => Some(k),
        _ => None,
    }
}

/// Lower a StyleIR into a `const fn name() -> taffy::Style` definition.
///
/// Rect-kind properties (margin/padding) appear as four consecutive entries
/// in t/r/b/l order; `gap` as two (row, col) — the lig-style convention.
// One arm per schema field keeps the mapping auditable against §8.2.
#[must_use]
pub fn lower_to_taffy_const_fn(ir: &StyleIr, name: &str) -> LoweredStyle {
    use std::collections::BTreeMap;

    let mut assigns: Vec<String> = Vec::new();
    let mut unmapped: Vec<String> = Vec::new();
    let push = |field: &str,
                value: String,
                css: &str,
                assigns: &mut Vec<String>,
                unmapped: &mut Vec<String>| {
        assigns.push(format!("s.{field} = {value};"));
        let _ = css;
        let _ = unmapped;
    };

    // Group consecutive rect/list entries positionally: prop_id -> values.
    let mut groups: BTreeMap<u32, Vec<StyleValue>> = BTreeMap::new();
    let mut singles: Vec<(u32, StyleValue)> = Vec::new();
    {
        for (id, v) in &ir.longhands {
            let schema = &lig_style::schema::PROFILE_V1[id.get() as usize];
            let is_multi = matches!(
                schema.kind,
                lig_style::schema::ValueKind::Rect | lig_style::schema::ValueKind::List
            );
            if is_multi {
                groups.entry(id.get()).or_default().push(v.clone());
            } else {
                singles.push((id.get(), v.clone()));
            }
        }
    }

    // Rect/list groups first (structural fields), in schema order.
    for (id, vals) in &groups {
        let schema = &lig_style::schema::PROFILE_V1[*id as usize];
        let css = schema.css_name;
        let get4 = |vals: &[StyleValue],
                    f: fn(&StyleValue) -> Option<String>,
                    default: &str|
         -> [String; 4] {
            [0, 1, 2, 3].map(|i| {
                vals.get(i)
                    .and_then(f)
                    .unwrap_or_else(|| default.to_string())
            })
        };
        match schema.taffy_field {
            TaffyField::Margin { .. } => {
                let [t, r, b, l] = get4(vals, lpa, "taffy::LengthPercentageAuto::auto()");
                push(
                    "margin",
                    format!("taffy::Rect {{ top: {t}, right: {r}, bottom: {b}, left: {l} }}"),
                    css,
                    &mut assigns,
                    &mut unmapped,
                );
            }
            TaffyField::Padding { .. } => {
                let [t, r, b, l] = get4(vals, lp, "taffy::LengthPercentage::length(0.0)");
                push(
                    "padding",
                    format!("taffy::Rect {{ top: {t}, right: {r}, bottom: {b}, left: {l} }}"),
                    css,
                    &mut assigns,
                    &mut unmapped,
                );
            }
            TaffyField::Gap { .. } => {
                let w = vals
                    .first()
                    .and_then(lp)
                    .unwrap_or_else(|| "taffy::LengthPercentage::length(0.0)".to_string());
                let h = vals
                    .get(1)
                    .and_then(lp)
                    .unwrap_or_else(|| "taffy::LengthPercentage::length(0.0)".to_string());
                push(
                    "gap",
                    format!("taffy::Size {{ width: {w}, height: {h} }}"),
                    css,
                    &mut assigns,
                    &mut unmapped,
                );
            }
            _ => unmapped.push(css.to_string()),
        }
    }

    // Single-value properties, in schema order.
    for (id, v) in singles {
        let schema = &lig_style::schema::PROFILE_V1[id as usize];
        let css = schema.css_name;
        match schema.taffy_field {
            TaffyField::Display => match keyword(&v) {
                Some("flex") => push(
                    "display",
                    "taffy::style::Display::Flex".into(),
                    css,
                    &mut assigns,
                    &mut unmapped,
                ),
                Some("block") => push(
                    "display",
                    "taffy::style::Display::Block".into(),
                    css,
                    &mut assigns,
                    &mut unmapped,
                ),
                Some("grid") => push(
                    "display",
                    "taffy::style::Display::Grid".into(),
                    css,
                    &mut assigns,
                    &mut unmapped,
                ),
                Some("none") => push(
                    "display",
                    "taffy::style::Display::None".into(),
                    css,
                    &mut assigns,
                    &mut unmapped,
                ),
                _ => unmapped.push(css.to_string()),
            },
            TaffyField::Position => match keyword(&v) {
                Some("static") => push(
                    "position",
                    "taffy::style::Position::Static".into(),
                    css,
                    &mut assigns,
                    &mut unmapped,
                ),
                Some("relative") => push(
                    "position",
                    "taffy::style::Position::Relative".into(),
                    css,
                    &mut assigns,
                    &mut unmapped,
                ),
                Some("absolute") => push(
                    "position",
                    "taffy::style::Position::Absolute".into(),
                    css,
                    &mut assigns,
                    &mut unmapped,
                ),
                _ => unmapped.push(css.to_string()),
            },
            TaffyField::Size { side } => {
                if let Some(d) = dimension(&v) {
                    let field = match side {
                        lig_style::schema::Side::Top => "s.size.width",
                        _ => "s.size.height",
                    };
                    assigns.push(format!("{field} = {d};"));
                } else {
                    unmapped.push(css.to_string());
                }
            }
            TaffyField::MinSize { side } => {
                if let Some(d) = lpa(&v) {
                    let field = match side {
                        lig_style::schema::Side::Top => "s.min_size.width",
                        _ => "s.min_size.height",
                    };
                    assigns.push(format!("{field} = {d};"));
                } else {
                    unmapped.push(css.to_string());
                }
            }
            TaffyField::MaxSize { side } => {
                if let Some(d) = lpa(&v) {
                    let field = match side {
                        lig_style::schema::Side::Top => "s.max_size.width",
                        _ => "s.max_size.height",
                    };
                    assigns.push(format!("{field} = {d};"));
                } else {
                    unmapped.push(css.to_string());
                }
            }
            TaffyField::AspectRatio => match v {
                StyleValue::Num(n) => push(
                    "aspect_ratio",
                    format!("Some({})", fnum(n)),
                    css,
                    &mut assigns,
                    &mut unmapped,
                ),
                _ => unmapped.push(css.to_string()),
            },
            TaffyField::FlexDirection => match keyword(&v) {
                Some("row") => push(
                    "flex_direction",
                    "taffy::style::FlexDirection::Row".into(),
                    css,
                    &mut assigns,
                    &mut unmapped,
                ),
                Some("column") => push(
                    "flex_direction",
                    "taffy::style::FlexDirection::Column".into(),
                    css,
                    &mut assigns,
                    &mut unmapped,
                ),
                Some("row-reverse") => push(
                    "flex_direction",
                    "taffy::style::FlexDirection::RowReverse".into(),
                    css,
                    &mut assigns,
                    &mut unmapped,
                ),
                Some("column-reverse") => push(
                    "flex_direction",
                    "taffy::style::FlexDirection::ColumnReverse".into(),
                    css,
                    &mut assigns,
                    &mut unmapped,
                ),
                _ => unmapped.push(css.to_string()),
            },
            TaffyField::FlexWrap => match keyword(&v) {
                Some("nowrap") => push(
                    "flex_wrap",
                    "taffy::style::FlexWrap::NoWrap".into(),
                    css,
                    &mut assigns,
                    &mut unmapped,
                ),
                Some("wrap") => push(
                    "flex_wrap",
                    "taffy::style::FlexWrap::Wrap".into(),
                    css,
                    &mut assigns,
                    &mut unmapped,
                ),
                Some("wrap-reverse") => push(
                    "flex_wrap",
                    "taffy::style::FlexWrap::WrapReverse".into(),
                    css,
                    &mut assigns,
                    &mut unmapped,
                ),
                _ => unmapped.push(css.to_string()),
            },
            TaffyField::FlexGrow => {
                if let StyleValue::Num(n) = v {
                    push("flex_grow", fnum(n), css, &mut assigns, &mut unmapped);
                } else {
                    unmapped.push(css.to_string());
                }
            }
            TaffyField::FlexShrink => {
                if let StyleValue::Num(n) = v {
                    push("flex_shrink", fnum(n), css, &mut assigns, &mut unmapped);
                } else {
                    unmapped.push(css.to_string());
                }
            }
            TaffyField::FlexBasis => {
                if let Some(d) = dimension(&v) {
                    push("flex_basis", d, css, &mut assigns, &mut unmapped);
                } else {
                    unmapped.push(css.to_string());
                }
            }
            TaffyField::AlignItems => match align_kw(&v) {
                Some(k) => push("align_items", k, css, &mut assigns, &mut unmapped),
                None => unmapped.push(css.to_string()),
            },
            TaffyField::AlignSelf => match align_kw(&v) {
                Some(k) => push(
                    "align_self",
                    format!("Some({k})"),
                    css,
                    &mut assigns,
                    &mut unmapped,
                ),
                None => unmapped.push(css.to_string()),
            },
            TaffyField::JustifyContent => {
                match content_kw(&v).map(|s| s.replace("AlignContent", "JustifyContent")) {
                    Some(k) => push("justify_content", k, css, &mut assigns, &mut unmapped),
                    None => unmapped.push(css.to_string()),
                }
            }
            TaffyField::AlignContent => match content_kw(&v) {
                Some(k) => push("align_content", k, css, &mut assigns, &mut unmapped),
                None => unmapped.push(css.to_string()),
            },
            _ => unmapped.push(css.to_string()),
        }
    }

    let body = assigns.join("\n    ");
    let const_fn_text = format!(
        "pub const fn {name}() -> taffy::Style {{\n    let mut s = taffy::Style::DEFAULT;\n    {body}\n    s\n}}"
    );
    LoweredStyle {
        const_fn_text,
        fn_name: name.to_string(),
        unmapped,
    }
}

fn align_kw(v: &StyleValue) -> Option<String> {
    let k = keyword(v)?;
    Some(match k {
        "normal" => "taffy::style::AlignItems::NORMAL".to_string(),
        "start" => "taffy::style::AlignItems::START".to_string(),
        "end" => "taffy::style::AlignItems::END".to_string(),
        "center" => "taffy::style::AlignItems::CENTER".to_string(),
        "stretch" => "taffy::style::AlignItems::STRETCH".to_string(),
        "baseline" => "taffy::style::AlignItems::BASELINE".to_string(),
        "flex-start" => "taffy::style::AlignItems::FLEX_START".to_string(),
        "flex-end" => "taffy::style::AlignItems::FLEX_END".to_string(),
        _ => return None,
    })
}

fn content_kw(v: &StyleValue) -> Option<String> {
    let k = keyword(v)?;
    Some(match k {
        "normal" => "taffy::style::AlignContent::NORMAL".to_string(),
        "start" => "taffy::style::AlignContent::START".to_string(),
        "end" => "taffy::style::AlignContent::END".to_string(),
        "center" => "taffy::style::AlignContent::CENTER".to_string(),
        "stretch" => "taffy::style::AlignContent::STRETCH".to_string(),
        "space-between" => "taffy::style::AlignContent::SPACE_BETWEEN".to_string(),
        "space-around" => "taffy::style::AlignContent::SPACE_AROUND".to_string(),
        "space-evenly" => "taffy::style::AlignContent::SPACE_EVENLY".to_string(),
        "flex-start" => "taffy::style::AlignContent::FLEX_START".to_string(),
        "flex-end" => "taffy::style::AlignContent::FLEX_END".to_string(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::lower_to_taffy_const_fn;
    use lig_core::span::Span;
    use lig_style::parse_declarations;

    const SPAN: Span = Span::PLACEHOLDER;

    #[test]
    fn emits_display_and_size_assignments() {
        let ir = parse_declarations("display:flex;width:100px", SPAN).expect("ok");
        let out = lower_to_taffy_const_fn(&ir, "style_0");
        assert!(
            out.const_fn_text
                .contains("s.display = taffy::style::Display::Flex;"),
            "{}",
            out.const_fn_text
        );
        assert!(
            out.const_fn_text
                .contains("s.size.width = taffy::Dimension::length(100.0);"),
            "{}",
            out.const_fn_text
        );
        assert!(
            out.const_fn_text
                .starts_with("pub const fn style_0() -> taffy::Style {")
        );
        assert_eq!(out.unmapped.len(), 0, "nothing unmapped");
    }

    #[test]
    fn padding_expands_to_four_sides() {
        let ir = parse_declarations("padding:16px", SPAN).expect("ok");
        let out = lower_to_taffy_const_fn(&ir, "style_1");
        assert!(
            out.const_fn_text.contains("s.padding = taffy::Rect { top: taffy::LengthPercentage::length(16.0), right: taffy::LengthPercentage::length(16.0), bottom: taffy::LengthPercentage::length(16.0), left: taffy::LengthPercentage::length(16.0) };"),
            "{}",
            out.const_fn_text
        );
    }

    #[test]
    fn unmapped_values_are_listed_never_dropped() {
        // Schema-known but unmappable keyword → emitter-level `unmapped`.
        let ir = parse_declarations("display:table", SPAN).expect("ok");
        let out = lower_to_taffy_const_fn(&ir, "style_2");
        assert_eq!(out.unmapped, vec!["display".to_string()]);
        // Schema-unknown properties never reach the emitter: they stay in the
        // parse-level extras bag (§8.2 passthrough).
        let ir2 = parse_declarations("display:flex;color:#00f", SPAN).expect("ok");
        assert_eq!(ir2.extras, vec![("color".to_string(), "#00f".to_string())]);
    }

    #[test]
    fn alignment_keywords_map_to_consts() {
        let ir =
            parse_declarations("align-items:center;justify-content:flex-end", SPAN).expect("ok");
        let out = lower_to_taffy_const_fn(&ir, "style_3");
        assert!(
            out.const_fn_text
                .contains("s.align_items = taffy::style::AlignItems::CENTER;"),
            "{}",
            out.const_fn_text
        );
        assert!(
            out.const_fn_text
                .contains("s.justify_content = taffy::style::JustifyContent::FLEX_END;"),
            "{}",
            out.const_fn_text
        );
    }

    #[test]
    fn percentages_convert_to_unit_interval() {
        let ir = parse_declarations("width:50%", SPAN).expect("ok");
        let out = lower_to_taffy_const_fn(&ir, "style_4");
        assert!(
            out.const_fn_text.contains("taffy::Dimension::percent(0.5)"),
            "{}",
            out.const_fn_text
        );
    }
}
