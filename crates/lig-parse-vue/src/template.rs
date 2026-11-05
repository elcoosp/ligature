//! The admitted `<template>` grammar (§6.2): a hand-rolled, fail-closed
//! parser over the raw template text. Absolute byte ranges throughout — the
//! template lives inside the `.vue` source, so every diagnostic and every
//! lowered expression keeps its true source position.
//!
//! Admitted: elements, static attributes, `:k`/`v-bind:k` bindings,
//! `@ev`/`v-on:ev` handlers, `v-if`/`v-else-if`/`v-else`, `v-for`,
//! `v-html`, text with `{{ … }}` interpolations, HTML comments. Everything
//! else (and every syntax error) is `LIG-E001`.

use lig_core::diagnostic::LigDiagnostic;
use lig_core::span::Span;

/// A parsed template node.
#[derive(Debug, Clone)]
pub enum TplNode {
    /// An element with attributes and children.
    Elem(TplElement),
    /// Text with optional `{{ … }}` holes.
    Text(TplText),
}

/// An element.
#[derive(Debug, Clone)]
pub struct TplElement {
    /// Tag name (case preserved; PascalCase ⇒ component).
    pub tag: String,
    /// Attributes/directives in source order.
    pub attrs: Vec<TplAttr>,
    /// Child nodes (comments removed, whitespace untouched — trimming is a
    /// lowering decision, matching the Solid frontend's text rules).
    pub children: Vec<TplNode>,
    /// Whole-element span.
    pub span: (usize, usize),
}

/// A text node.
#[derive(Debug, Clone)]
pub struct TplText {
    /// Static and hole parts, in order.
    pub parts: Vec<TplTextPart>,
    /// Whole-node span.
    pub span: (usize, usize),
}

/// One part of a text node.
#[derive(Debug, Clone)]
pub enum TplTextPart {
    /// Static text range.
    Str((usize, usize)),
    /// `{{ expr }}` — the expression's range (inside the braces).
    Hole((usize, usize)),
}

/// An attribute or directive.
#[derive(Debug, Clone)]
pub enum TplAttr {
    /// Static attribute `k="v"` (or bare boolean `k`).
    Static {
        /// Attribute name.
        name: String,
        /// Quoted value (`None` for bare attributes).
        value: Option<String>,
        /// Whole-attribute span.
        span: (usize, usize),
    },
    /// `:k="expr"` / `v-bind:k="expr"`.
    Bind {
        /// Bound attribute name.
        name: String,
        /// Expression range.
        expr: (usize, usize),
    },
    /// `@ev="stmt"` / `v-on:ev="stmt"`.
    On {
        /// Event name.
        event: String,
        /// Statement/expression range.
        stmt: (usize, usize),
    },
    /// `v-if="expr"`.
    If {
        /// Condition range.
        expr: (usize, usize),
    },
    /// `v-else-if="expr"`.
    ElseIf {
        /// Condition range.
        expr: (usize, usize),
    },
    /// `v-else`.
    Else,
    /// `v-for="item in list"` / `v-for="(item, index) in list"`.
    For {
        /// Item binding name.
        item: String,
        /// Optional index binding name.
        index: Option<String>,
        /// List expression range.
        list: (usize, usize),
    },
    /// `v-html="expr"`.
    Html {
        /// Expression range.
        expr: (usize, usize),
    },
}

/// HTML void elements: self-closing without `/>`.
const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

/// Parse one template body (absolute offsets into `source`).
///
/// # Errors
/// Fail-closed `LIG-E001` for every syntax error or non-admitted construct.
pub fn parse(source: &str, range: (usize, usize)) -> Result<Vec<TplNode>, Vec<LigDiagnostic>> {
    let mut p = Parser {
        src: source,
        bytes: source.as_bytes(),
        pos: range.0,
        end: range.1,
        diags: Vec::new(),
    };
    let children = p.parse_children()?;
    if p.diags.is_empty() {
        Ok(children)
    } else {
        Err(p.diags)
    }
}

impl TplNode {
    /// The node's element, if it is one.
    pub fn as_elem(&self) -> Option<&TplElement> {
        match self {
            TplNode::Elem(e) => Some(e),
            TplNode::Text(_) => None,
        }
    }

    /// The node's text, if it is one.
    pub fn as_text(&self) -> Option<&TplText> {
        match self {
            TplNode::Text(t) => Some(t),
            TplNode::Elem(_) => None,
        }
    }
}

struct Parser<'s> {
    src: &'s str,
    bytes: &'s [u8],
    pos: usize,
    end: usize,
    diags: Vec<LigDiagnostic>,
}

impl Parser<'_> {
    fn err(&mut self, start: usize, end: usize, msg: impl Into<String>) {
        self.diags.push(
            LigDiagnostic::error(
                "LIG-E001",
                msg.into(),
                Span::new(start as u32, end.min(self.src.len()) as u32),
            )
            .with_help("the admitted <template> grammar is documented in ADR-014"),
        );
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn starts(&self, s: &str) -> bool {
        self.src[self.pos..self.end.min(self.src.len())].starts_with(s)
    }

    fn skip_ws(&mut self) {
        while self.pos < self.end && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    /// Child nodes until the enclosing `</tag>` (or end of template).
    fn parse_children(&mut self) -> Result<Vec<TplNode>, Vec<LigDiagnostic>> {
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            if self.pos >= self.end {
                break;
            }
            if self.starts("<!--") {
                if let Some(off) = self.src[self.pos..self.end].find("-->") {
                    self.pos += off + 3;
                    continue;
                }
                self.err(self.pos, self.end, "unterminated HTML comment");
                break;
            }
            if self.starts("</") {
                // Enclosing close tag: the caller consumes it.
                break;
            }
            if self.peek() == Some(b'<') {
                match self.parse_element() {
                    Some(el) => out.push(TplNode::Elem(el)),
                    None => break,
                }
                continue;
            }
            match self.parse_text() {
                Some(t) => out.push(TplNode::Text(t)),
                None => break,
            }
        }
        if self.diags.is_empty() {
            Ok(out)
        } else {
            Err(std::mem::take(&mut self.diags))
        }
    }

    fn parse_element(&mut self) -> Option<TplElement> {
        let start = self.pos;
        self.pos += 1; // `<`
        let tag_start = self.pos;
        while self.pos < self.end
            && !self.bytes[self.pos].is_ascii_whitespace()
            && self.bytes[self.pos] != b'>'
            && !(self.bytes[self.pos] == b'/' && self.bytes.get(self.pos + 1) == Some(&b'>'))
        {
            self.pos += 1;
        }
        let tag = self.src[tag_start..self.pos].to_string();
        if tag.is_empty() {
            self.err(start, start + 1, "empty tag name");
            return None;
        }
        if tag == "template" || tag == "script" {
            self.err(
                start,
                self.pos,
                format!("<{tag}> inside a template is outside the admitted subset"),
            );
            return None;
        }
        let mut attrs = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                None => {
                    self.err(start, self.end, format!("unclosed <{tag}> tag"));
                    return None;
                }
                Some(b'>') => {
                    self.pos += 1;
                    // HTML void elements close without `/>`.
                    if VOID_ELEMENTS.contains(&tag.as_str()) {
                        return Some(TplElement {
                            tag,
                            attrs,
                            children: Vec::new(),
                            span: (start, self.pos),
                        });
                    }
                    break;
                }
                Some(b'/') if self.bytes.get(self.pos + 1) == Some(&b'>') => {
                    self.pos += 2;
                    return Some(TplElement {
                        tag,
                        attrs,
                        children: Vec::new(),
                        span: (start, self.pos),
                    });
                }
                // A None here means the diagnostic is already recorded.
                _ => attrs.push(self.parse_attr()?),
            }
        }
        let children = match self.parse_children() {
            Ok(c) => c,
            Err(diags) => {
                self.diags.extend(diags);
                return None;
            }
        };
        // Consume the matching close tag.
        let close = format!("</{tag}");
        if self.starts(&close) {
            self.pos += close.len();
            while self.pos < self.end && self.bytes[self.pos] != b'>' {
                self.pos += 1;
            }
            self.pos = (self.pos + 1).min(self.end);
        } else {
            self.err(
                start,
                self.pos,
                format!("<{tag}> is never closed (or closed out of order)"),
            );
            return None;
        }
        Some(TplElement {
            tag,
            attrs,
            children,
            span: (start, self.pos),
        })
    }

    fn parse_attr(&mut self) -> Option<TplAttr> {
        let start = self.pos;
        // Directive prefixes (longest first).
        for (prefix, kind) in [
            ("v-else-if", 0usize),
            ("v-else", 1),
            ("v-if", 2),
            ("v-for", 3),
            ("v-html", 4),
            ("v-bind:", 5),
            ("v-on:", 6),
            (":", 7),
            ("@", 8),
        ] {
            if self.starts(prefix) {
                return self.parse_directive(prefix, kind, start);
            }
        }
        // Unknown v-* directives fail closed (ADR-014).
        if self.starts("v-") {
            let start = self.pos;
            let mut end = self.pos;
            while end < self.end
                && !self.bytes[end].is_ascii_whitespace()
                && self.bytes[end] != b'='
                && self.bytes[end] != b'>'
            {
                end += 1;
            }
            self.err(
                start,
                end,
                format!(
                    "directive `{}` is outside the admitted subset",
                    &self.src[start..end]
                ),
            );
            return None;
        }
        // Plain attribute name.
        let name_start = self.pos;
        while self.pos < self.end
            && !self.bytes[self.pos].is_ascii_whitespace()
            && self.bytes[self.pos] != b'='
            && self.bytes[self.pos] != b'>'
            && !(self.bytes[self.pos] == b'/' && self.bytes.get(self.pos + 1) == Some(&b'>'))
        {
            self.pos += 1;
        }
        let name = self.src[name_start..self.pos].to_string();
        if name.is_empty() {
            self.err(start, start + 1, "expected an attribute");
            return None;
        }
        // Optional `="value"`.
        if self.peek() == Some(b'=') {
            self.pos += 1;
            let (value, _vstart, vend) = self.read_quoted(start)?;
            Some(TplAttr::Static {
                name,
                value: Some(value),
                span: (start, vend + 1),
            })
        } else {
            Some(TplAttr::Static {
                name,
                value: None,
                span: (start, self.pos),
            })
        }
    }

    /// A directive after its prefix was matched.
    fn parse_directive(&mut self, prefix: &str, kind: usize, start: usize) -> Option<TplAttr> {
        self.pos += prefix.len();
        let arg_start = self.pos;
        while self.pos < self.end
            && !self.bytes[self.pos].is_ascii_whitespace()
            && self.bytes[self.pos] != b'='
            && self.bytes[self.pos] != b'>'
        {
            self.pos += 1;
        }
        let arg = self.src[arg_start..self.pos].to_string();
        if kind == 1 {
            return Some(TplAttr::Else); // v-else
        }
        if self.peek() != Some(b'=') {
            self.err(
                start,
                self.pos,
                format!("`{prefix}` requires a quoted value"),
            );
            return None;
        }
        self.pos += 1;
        let (value, vstart, vend) = self.read_quoted(start)?;
        let expr = (vstart, vend);
        Some(match kind {
            0 => TplAttr::ElseIf { expr },
            2 => TplAttr::If { expr },
            3 => self.parse_for(&value, vstart)?,
            4 => TplAttr::Html { expr },
            // 5/6: `v-bind:`/`v-on:`; 7/8: the `:`/`@` shorthands.
            5 | 7 => TplAttr::Bind { name: arg, expr },
            6 | 8 => TplAttr::On {
                event: arg,
                stmt: expr,
            },
            _ => unreachable!("directive kind"),
        })
    }

    /// `v-for="item in list"` / `(item, index) in list` / `… of …`.
    /// `vstart` is the absolute offset of the quoted value's first byte.
    fn parse_for(&mut self, value: &str, vstart: usize) -> Option<TplAttr> {
        let (head_txt, list_rel) = if let Some(i) = value.find(" in ") {
            (&value[..i], i + 4)
        } else if let Some(i) = value.find(" of ") {
            (&value[..i], i + 4)
        } else {
            self.err(
                vstart,
                vstart + value.len(),
                "`v-for` requires `item in list` (or `of`)",
            );
            return None;
        };
        let head = head_txt.trim();
        let list_trimmed = value[list_rel..].trim();
        let list_leading = value[list_rel..].len() - value[list_rel..].trim_start().len();
        let list_range = (
            vstart + list_rel + list_leading,
            vstart + list_rel + list_leading + list_trimmed.len(),
        );
        let (item, index) =
            if let Some(inner) = head.strip_prefix('(').and_then(|h| h.strip_suffix(')')) {
                let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
                if parts.len() == 2 {
                    (parts[0].to_string(), Some(parts[1].to_string()))
                } else if parts.len() == 1 {
                    (parts[0].to_string(), None)
                } else {
                    self.err(vstart, list_range.1, "`v-for` admits (item, index) only");
                    return None;
                }
            } else {
                (head.to_string(), None)
            };
        if item.is_empty() {
            self.err(vstart, list_range.1, "`v-for` requires an item binding");
            return None;
        }
        Some(TplAttr::For {
            item,
            index,
            list: list_range,
        })
    }

    /// Read a quoted value after `=`.
    /// Returns (value, absolute start, absolute end) of the content.
    fn read_quoted(&mut self, attr_start: usize) -> Option<(String, usize, usize)> {
        let Some(q @ (b'"' | b'\'')) = self.peek() else {
            self.err(
                attr_start,
                self.pos,
                "attribute values must be quoted (\"…\" or '…')",
            );
            return None;
        };
        self.pos += 1;
        let vstart = self.pos;
        while self.pos < self.end && self.bytes[self.pos] != q {
            self.pos += 1;
        }
        if self.pos >= self.end {
            self.err(attr_start, self.end, "unterminated attribute value");
            return None;
        }
        let value = self.src[vstart..self.pos].to_string();
        let vend = self.pos;
        self.pos += 1;
        Some((value, vstart, vend))
    }

    /// Text up to the next `<`, with `{{ … }}` holes.
    fn parse_text(&mut self) -> Option<TplText> {
        let start = self.pos;
        let mut parts = Vec::new();
        let mut seg_start = self.pos;
        loop {
            if self.pos >= self.end || self.peek() == Some(b'<') {
                break;
            }
            if self.starts("{{") {
                if self.pos > seg_start {
                    parts.push(TplTextPart::Str((seg_start, self.pos)));
                }
                self.pos += 2;
                let expr_start = self.pos;
                if let Some(off) = self.src[self.pos..self.end].find("}}") {
                    let expr_end = self.pos + off;
                    parts.push(TplTextPart::Hole((expr_start, expr_end)));
                    self.pos = expr_end + 2;
                    seg_start = self.pos;
                } else {
                    self.err(self.pos, self.end, "unterminated {{ … }} interpolation");
                    return None;
                }
                continue;
            }
            self.pos += 1;
        }
        if self.pos > seg_start {
            parts.push(TplTextPart::Str((seg_start, self.pos)));
        }
        if parts.is_empty() {
            return None;
        }
        Some(TplText {
            parts,
            span: (start, self.pos),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{TplAttr, TplNode, TplTextPart, parse};

    fn attr_names(nodes: &[TplNode]) -> Vec<String> {
        nodes
            .iter()
            .filter_map(|n| n.as_elem())
            .flat_map(|e| {
                e.attrs
                    .iter()
                    .map(|a| match a {
                        TplAttr::Static { name, .. } => format!("static:{name}"),
                        TplAttr::Bind { name, .. } => format!("bind:{name}"),
                        TplAttr::On { event, .. } => format!("on:{event}"),
                        TplAttr::If { .. } => "if".to_string(),
                        TplAttr::ElseIf { .. } => "else-if".to_string(),
                        TplAttr::Else => "else".to_string(),
                        TplAttr::For { .. } => "for".to_string(),
                        TplAttr::Html { .. } => "html".to_string(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[test]
    fn parses_elements_attrs_and_text() {
        let src = "<div class=\"a\"><img :src=\"url\" alt=\"x\" /><p>hi {{ name }}</p></div>";
        let nodes = parse(src, (0, src.len())).expect("parses");
        assert_eq!(attr_names(&nodes), vec!["static:class"]);
        let div = nodes[0].as_elem().expect("div");
        assert_eq!(div.children.len(), 2);
        let img = div.children[0].as_elem().expect("img");
        assert_eq!(img.attrs.len(), 2);
        let p = div.children[1].as_elem().expect("p");
        let t = p.children[0].as_text().expect("text");
        assert_eq!(t.parts.len(), 2);
        assert!(matches!(t.parts[0], TplTextPart::Str(_)));
        assert!(matches!(t.parts[1], TplTextPart::Hole(_)));
    }

    #[test]
    fn parses_directives() {
        let src = "<b v-if=\"on\">x</b><i v-else>no</i><li v-for=\"(n, i) in rows\">{{ i }}</li><div v-html=\"h\"/>";
        let nodes = parse(src, (0, src.len())).expect("parses");
        let names = attr_names(&nodes);
        assert_eq!(names, vec!["if", "else", "for", "html"]);
    }

    #[test]
    fn syntax_errors_are_fail_closed() {
        let bad = [
            "<div class=>x</div>",
            "<div><p>unclosed",
            "<p v-model=\"x\"/></p>",
            "<template><p/></template>",
        ];
        for src in bad {
            assert!(
                parse(src, (0, src.len())).is_err(),
                "expected rejection: {src}"
            );
        }
    }
}
