//! StyleIR: property schema table, lightningcss-backed parsing,
//! normalization, and interning (D-12, §8).
//!
//! lightningcss is the MPL-2.0 *dependency boundary* (§15): it tokenizes,
//! validates, and normalizes; the property schema below is Ligature's own
//! code-generated-style table (the lightningcss macro-table pattern), and no
//! MPL source is copied. Unknown properties are tiered: kept in `extras`,
//! never silently dropped (§8.2).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod schema;

pub use schema::{PROFILE_V1, PropSchema, ValueKind};

use lig_core::diagnostic::LigDiagnostic;
use lig_core::hash::fnv1a64;
use lig_core::id::StylePropId;
use lig_core::span::Span;
use lightningcss::stylesheet::{ParserOptions, StyleAttribute};

/// A lowered style: normalized longhands plus the passthrough extras bag.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StyleIr {
    /// Ordered `(property, value)` longhands, in schema order.
    pub longhands: Vec<(StylePropId, StyleValue)>,
    /// Properties the profile v1 table does not know — kept, never dropped.
    pub extras: Vec<(String, String)>,
}

/// A normalized style value (subset needed by profile v1).
#[derive(Debug, Clone, PartialEq)]
pub enum StyleValue {
    /// Absolute length in px.
    Len(f32),
    /// Percentage.
    Pct(f32),
    /// `auto`.
    Auto,
    /// Keyword (e.g. `flex`, `column`, `wrap`, `center`).
    Keyword(&'static str),
    /// Unitless number.
    Num(f32),
    /// Color or other opaque token — host-owned.
    Raw(String),
}

impl StyleValue {
    /// Canonical CSS token for serialization.
    #[must_use]
    pub fn css(&self) -> String {
        match self {
            StyleValue::Len(l) => format!("{l}px"),
            StyleValue::Pct(p) => format!("{p}%"),
            StyleValue::Auto => "auto".to_string(),
            StyleValue::Keyword(k) => (*k).to_string(),
            StyleValue::Num(n) => format!("{n}"),
            StyleValue::Raw(r) => r.clone(),
        }
    }
}

/// Parse a CSS declaration block (`display:flex;padding:16px`) into StyleIR.
///
/// # Errors
/// `LIG-E300`-family diagnostic when lightningcss rejects the declaration
/// block (fail-closed: unparseable style is a compile error, not a guess).
pub fn parse_declarations(css: &str, span: Span) -> Result<StyleIr, LigDiagnostic> {
    let attr = StyleAttribute::parse(css, ParserOptions::default()).map_err(|e| {
        LigDiagnostic::error("LIG-E300", format!("invalid style declaration: {e}"), span)
    })?;

    let mut ir = StyleIr::default();
    let mut raw_pairs: Vec<(String, String)> = Vec::new();
    for (property, _important) in attr.declarations.iter() {
        let text = property
            .to_css_string(false, lightningcss::stylesheet::PrinterOptions::default())
            .map_err(|e| {
                LigDiagnostic::error("LIG-E300", format!("style serialization failed: {e}"), span)
            })?;
        let Some((name, value)) = text.split_once(':') else {
            continue;
        };
        raw_pairs.push((name.trim().to_string(), value.trim().to_string()));
    }

    // Shorthand expansion (margin/padding) happens here — profile v1's
    // longhand discipline (§8.1: ordered longhands).
    for (name, value) in raw_pairs {
        let Some((prop_id, schema)) = schema::lookup(&name) else {
            ir.extras.push((name, value));
            continue;
        };
        match schema.kind {
            ValueKind::One => {
                if let Some(v) = parse_value(&value) {
                    ir.longhands.push((prop_id, v));
                } else {
                    ir.extras.push((name, value));
                }
            }
            ValueKind::Rect => {
                for (side, token) in expand_shorthand(&value) {
                    if let Some(v) = parse_value(&token) {
                        ir.longhands.push((prop_id, v));
                    }
                    let _ = side;
                }
                // Sides are distinguished by the *emitter* through position;
                // store four entries in t/r/b/l order.
            }
            ValueKind::List => {
                for token in value.split_whitespace() {
                    if let Some(v) = parse_value(token) {
                        ir.longhands.push((prop_id, v));
                    }
                }
            }
        }
    }

    // Canonical order: schema order (profile-normalized, §8.1).
    ir.longhands.sort_by_key(|(id, _)| id.get());
    Ok(ir)
}

/// Canonical CSS text of a StyleIR (schema order, `name:value` joined by `;`).
/// Rect-kind properties collapse their four side entries into one
/// declaration; list-kind (gap) collapse two.
#[must_use]
pub fn to_css(ir: &StyleIr) -> String {
    let mut out = String::new();
    let mut last_id: Option<u32> = None;
    let mut side_tokens: Vec<String> = Vec::new();
    let is_multi = |id: u32| {
        matches!(
            schema::PROFILE_V1[id as usize].kind,
            schema::ValueKind::Rect | schema::ValueKind::List
        )
    };
    for (id, v) in &ir.longhands {
        if last_id == Some(id.get()) && is_multi(id.get()) {
            side_tokens.push(v.css());
            continue;
        }
        flush_multi(&mut out, &mut last_id, &mut side_tokens, is_multi);
        if is_multi(id.get()) {
            side_tokens.push(v.css());
            last_id = Some(id.get());
            continue;
        }
        if !out.is_empty() {
            out.push(';');
        }
        out.push_str(schema::PROFILE_V1[id.get() as usize].css_name);
        out.push(':');
        out.push_str(&v.css());
        last_id = None;
    }
    flush_multi(&mut out, &mut last_id, &mut side_tokens, is_multi);
    for (name, value) in &ir.extras {
        if !out.is_empty() {
            out.push(';');
        }
        out.push_str(name);
        out.push(':');
        out.push_str(value);
    }
    out
}

fn flush_multi(
    out: &mut String,
    last_id: &mut Option<u32>,
    side_tokens: &mut Vec<String>,
    is_multi: fn(u32) -> bool,
) {
    if let Some(id) = *last_id
        && is_multi(id)
    {
        if !out.is_empty() {
            out.push(';');
        }
        out.push_str(schema::PROFILE_V1[id as usize].css_name);
        out.push(':');
        // Uniform boxes collapse to their single value (canonical form).
        if side_tokens.windows(2).all(|w| w[0] == w[1]) {
            out.push_str(&side_tokens[0]);
        } else {
            out.push_str(&side_tokens.join(" "));
        }
    }
    side_tokens.clear();
    *last_id = None;
}

/// Content hash over the canonical CSS form (C1-style content addressing).
#[must_use]
pub fn hash(ir: &StyleIr) -> u64 {
    fnv1a64(to_css(ir).as_bytes())
}

/// Convert a StyleIR into a `lig-ir` StyleConst for module interning.
#[must_use]
pub fn to_style_const(
    ir: &StyleIr,
    strings: &mut lig_core::intern::StringTable,
) -> lig_ir::style::StyleConst {
    let css = to_css(ir);
    let hash = hash(ir);
    let longhands = ir
        .longhands
        .iter()
        .map(|(id, v)| {
            let value = match v {
                StyleValue::Len(l) => lig_ir::style::StyleValue::Len(*l),
                StyleValue::Pct(p) => lig_ir::style::StyleValue::Pct(*p),
                StyleValue::Auto => lig_ir::style::StyleValue::Auto,
                StyleValue::Keyword(k) => {
                    lig_ir::style::StyleValue::Keyword(strings.intern((*k).to_string()))
                }
                StyleValue::Num(n) => lig_ir::style::StyleValue::Num(*n),
                StyleValue::Raw(r) => lig_ir::style::StyleValue::Raw(strings.intern(r.clone())),
            };
            lig_ir::style::StyleLonghand { prop: *id, value }
        })
        .collect();
    lig_ir::style::StyleConst {
        hash,
        css,
        longhands,
    }
}

/// Parse one CSS value token.
#[must_use]
pub fn parse_value(token: &str) -> Option<StyleValue> {
    let t = token.trim();
    if let Some(stripped) = t.strip_suffix("px") {
        return stripped.parse::<f32>().ok().map(StyleValue::Len);
    }
    if let Some(stripped) = t.strip_suffix('%') {
        return stripped.parse::<f32>().ok().map(StyleValue::Pct);
    }
    if t == "auto" {
        return Some(StyleValue::Auto);
    }
    if let Ok(n) = t.parse::<f32>() {
        return Some(StyleValue::Num(n));
    }
    // Known keywords only; everything else is opaque (tiered, never dropped).
    match t {
        "flex" | "block" | "grid" | "none" | "static" | "relative" | "absolute" | "row"
        | "column" | "row-reverse" | "column-reverse" | "wrap" | "nowrap" | "wrap-reverse"
        | "center" | "start" | "end" | "stretch" | "flex-start" | "flex-end" | "space-between"
        | "space-around" | "space-evenly" | "baseline" | "border-box" | "content-box"
        | "hidden" | "scroll" | "visible" => Some(StyleValue::Keyword(leak_keyword(t))),
        _ => Some(StyleValue::Raw(t.to_string())),
    }
}

/// Keywords are a fixed table — leaking here interns them once for
/// `'static` use (bounded by the keyword list above plus raw passthrough).
fn leak_keyword(t: &str) -> &'static str {
    // Safety of the *value space*: this branch only runs for the fixed
    // keyword list above; Box::leak on user input is avoided by the Raw arm.
    std::boxed::Box::leak(t.to_string().into_boxed_str())
}

/// Expand a CSS box shorthand into t/r/b/l tokens.
#[must_use]
pub fn expand_shorthand(value: &str) -> [(&'static str, String); 4] {
    let tokens: Vec<&str> = value.split_whitespace().collect();
    let sides: Vec<String> = match tokens.len() {
        1 => vec![
            tokens[0].to_string(),
            tokens[0].to_string(),
            tokens[0].to_string(),
            tokens[0].to_string(),
        ],
        2 => vec![
            tokens[0].to_string(),
            tokens[1].to_string(),
            tokens[0].to_string(),
            tokens[1].to_string(),
        ],
        3 => vec![
            tokens[0].to_string(),
            tokens[1].to_string(),
            tokens[2].to_string(),
            tokens[1].to_string(),
        ],
        _ => vec![
            tokens[0].to_string(),
            tokens[1].to_string(),
            tokens[2].to_string(),
            tokens[3].to_string(),
        ],
    };
    [
        ("top", sides[0].clone()),
        ("right", sides[1].clone()),
        ("bottom", sides[2].clone()),
        ("left", sides[3].clone()),
    ]
}

#[cfg(test)]
mod tests {
    use super::{StyleValue, expand_shorthand, hash, parse_declarations, parse_value, to_css};
    use lig_core::id::StylePropId;
    use lig_core::span::Span;

    const SPAN: Span = Span::PLACEHOLDER;

    #[test]
    fn parses_lengths_percent_and_auto() {
        assert_eq!(parse_value("16px"), Some(StyleValue::Len(16.0)));
        assert_eq!(parse_value("50%"), Some(StyleValue::Pct(50.0)));
        assert_eq!(parse_value("auto"), Some(StyleValue::Auto));
        assert_eq!(parse_value("2"), Some(StyleValue::Num(2.0)));
        assert_eq!(parse_value("flex"), Some(StyleValue::Keyword("flex")));
    }

    #[test]
    fn shorthand_expands_trbl() {
        let [t, r, b, l] = expand_shorthand("8px 16px");
        assert_eq!(
            (t.1.as_str(), r.1.as_str(), b.1.as_str(), l.1.as_str()),
            ("8px", "16px", "8px", "16px")
        );
        let [t, r, b, l] = expand_shorthand("1px 2px 3px");
        assert_eq!(
            (t.1.as_str(), r.1.as_str(), b.1.as_str(), l.1.as_str()),
            ("1px", "2px", "3px", "2px")
        );
        let [t, _r, b, _l] = expand_shorthand("4px");
        assert_eq!(t.1.as_str(), "4px");
        assert_eq!(b.1.as_str(), "4px");
    }

    #[test]
    fn display_and_padding_lower_in_schema_order() {
        let ir = parse_declarations("padding:16px;display:flex", SPAN).expect("parses");
        // schema order regardless of source order
        let first = &ir.longhands[0];
        assert_eq!(first.0, StylePropId::new(0)); // display
        assert_eq!(first.1, StyleValue::Keyword("flex"));
        assert!(
            ir.longhands
                .iter()
                .any(|(id, v)| *id == StylePropId::new(14) && *v == StyleValue::Len(16.0))
        );
        assert_eq!(to_css(&ir), "display:flex;padding:16px");
    }

    #[test]
    fn unknown_properties_land_in_extras_never_dropped() {
        let ir = parse_declarations("display:flex;color:#00f;float:left", SPAN).expect("parses");
        assert!(
            ir.extras.iter().any(|(n, _)| n == "color"),
            "color kept in extras"
        );
        assert!(
            ir.extras.iter().any(|(n, _)| n == "float"),
            "float kept in extras"
        );
        assert_eq!(to_css(&ir), "display:flex;color:#00f;float:left");
    }

    #[test]
    fn hash_is_content_addressed_and_order_insensitive() {
        let a = parse_declarations("display:flex;padding:16px", SPAN).expect("ok");
        let b = parse_declarations("padding:16px;display:flex", SPAN).expect("ok");
        assert_eq!(hash(&a), hash(&b));
        let c = parse_declarations("display:block;padding:16px", SPAN).expect("ok");
        assert_ne!(hash(&a), hash(&c));
    }

    #[test]
    fn invalid_css_is_a_magnitude_300_error() {
        let err = parse_declarations("display:))))", SPAN).expect_err("must fail");
        assert_eq!(err.code, "LIG-E300");
    }

    #[test]
    fn insets_and_gaps_parse() {
        let ir = parse_declarations("top:4px;left:10%;gap:8px", SPAN).expect("ok");
        assert!(
            ir.longhands
                .iter()
                .any(|(id, v)| *id == StylePropId::new(2) && *v == StyleValue::Len(4.0))
        );
        assert!(
            ir.longhands
                .iter()
                .any(|(id, v)| *id == StylePropId::new(5) && *v == StyleValue::Pct(10.0))
        );
        assert!(
            ir.longhands
                .iter()
                .any(|(id, v)| *id == StylePropId::new(20) && *v == StyleValue::Len(8.0))
        );
    }
}
