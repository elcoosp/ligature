//! SFC block scanning (§6.2): split a `.vue` source into its admitted
//! top-level blocks.
//!
//! The admitted grammar (ADR-014): a `.vue` module is a sequence of one or
//! more SFC *sections*; each section is an optional `<script setup …>` block
//! followed by one `<template>` block. Single-section files are plain SFCs
//! (one component, named by the file stem). Multi-section files are the
//! fixture/module form for cross-component behavior — every section then
//! names its component with `defineOptions({ name: "…" })`. Anything else at
//! the top level is fail-closed (`LIG-E001`).

use lig_core::diagnostic::LigDiagnostic;
use lig_core::span::Span;

/// One SFC section: an optional script block and its template block, both
/// as byte ranges into the original source.
#[derive(Debug)]
pub struct Section {
    /// `<script setup>` content range (`None` for template-only sections).
    pub script: Option<(usize, usize)>,
    /// `<template>` content range.
    pub template: (usize, usize),
}

/// Scan a `.vue` source into admitted SFC sections.
///
/// # Errors
/// Fail-closed `LIG-E001` diagnostics for every non-admitted top-level shape
/// (missing setup, template without section, stray text, unterminated blocks).
pub fn scan(source: &str) -> Result<Vec<Section>, Vec<LigDiagnostic>> {
    let bytes = source.as_bytes();
    let mut diags: Vec<LigDiagnostic> = Vec::new();
    let mut sections: Vec<Section> = Vec::new();
    let mut i = 0usize;

    let err = |start: usize, end: usize, msg: &str| {
        LigDiagnostic::error(
            "LIG-E001",
            msg.to_string(),
            Span::new(start as u32, end as u32),
        )
    };

    while i < bytes.len() {
        // Whitespace is free; HTML comments between blocks are tolerated.
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if source[i..].starts_with("<!--") {
            if let Some(off) = source[i..].find("-->") {
                i += off + 3;
                continue;
            }
            diags.push(err(i, bytes.len(), "unterminated HTML comment"));
            break;
        }
        if source[i..].starts_with("<script") {
            // Opening tag: <script …> (must carry `setup`; lang must be ts).
            let Some(tag_end_rel) = source[i..].find('>') else {
                diags.push(err(i, bytes.len(), "unterminated <script> tag"));
                break;
            };
            let tag_end = i + tag_end_rel;
            let tag = &source[i..=tag_end];
            if !tag_attr_present(tag, "setup") {
                diags.push(err(
                    i,
                    tag_end + 1,
                    "only <script setup> is admitted (§6.2)",
                )
                .with_help("add the `setup` attribute — the Options API is outside the admitted subset"));
            }
            if let Some(lang) = tag_attr_value(tag, "lang")
                && lang != "ts"
            {
                diags.push(err(
                    i,
                    tag_end + 1,
                    "only lang=\"ts\" is admitted for <script setup>",
                ));
            }
            let Some(close_rel) = source[tag_end..].find("</script") else {
                diags.push(err(i, bytes.len(), "unterminated <script> block"));
                break;
            };
            let content_start = tag_end + 1;
            let content_end = tag_end + close_rel;
            // Consume the closing tag.
            let Some(gt) = source[content_end..].find('>') else {
                diags.push(err(content_end, bytes.len(), "unterminated </script> tag"));
                break;
            };
            i = content_end + gt + 1;

            // Each script opens a new section.
            sections.push(Section {
                script: Some((content_start, content_end)),
                template: (0, 0),
            });
            continue;
        }
        if source[i..].starts_with("<template") {
            let Some(tag_end_rel) = source[i..].find('>') else {
                diags.push(err(i, bytes.len(), "unterminated <template> tag"));
                break;
            };
            let tag_end = i + tag_end_rel;
            let Some(close_rel) = source[tag_end..].find("</template") else {
                diags.push(err(i, bytes.len(), "unterminated <template> block"));
                break;
            };
            let content_start = tag_end + 1;
            let content_end = tag_end + close_rel;
            let Some(gt) = source[content_end..].find('>') else {
                diags.push(err(
                    content_end,
                    bytes.len(),
                    "unterminated </template> tag",
                ));
                break;
            };
            i = content_end + gt + 1;

            match sections.last_mut() {
                Some(s) if s.template.1 == 0 => s.template = (content_start, content_end),
                Some(_) => {
                    diags.push(err(
                        i,
                        content_end,
                        "a <template> block must follow its <script setup> block",
                    ));
                }
                None => sections.push(Section {
                    script: None,
                    template: (content_start, content_end),
                }),
            }
            continue;
        }
        // Anything else at the top level is outside the admitted grammar.
        let end = source[i..]
            .find('>')
            .map_or(bytes.len(), |off| (i + off + 1).min(bytes.len()));
        diags.push(
            err(
                i,
                end,
                "top-level construct outside the admitted SFC grammar (§6.2)",
            )
            .with_help("admitted: <script setup lang=\"ts\"> and <template> blocks"),
        );
        i = end;
    }

    for (idx, s) in sections.iter().enumerate() {
        if s.script.is_none() && sections.len() > 1 {
            diags.push(err(
                0,
                1,
                "multi-section modules must give every section a <script setup> block (defineOptions naming)",
            ));
        }
        if s.template.1 == 0 {
            diags.push(err(0, 1, &format!("section {idx} has no <template> block")));
        }
    }
    if sections.is_empty() {
        diags.push(err(
            0,
            bytes.len().min(1),
            "no SFC sections found (admitted: <script setup lang=\"ts\"> + <template>)",
        ));
    }
    if diags.is_empty() {
        Ok(sections)
    } else {
        Err(diags)
    }
}

/// Whether a tag string carries a bare attribute (`setup`).
fn tag_attr_present(tag: &str, name: &str) -> bool {
    tag_attr_value(tag, name).is_some() || has_bare_attr(tag, name)
}

/// A quoted attribute value (`lang="ts"`), single- or double-quoted.
fn tag_attr_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let body = tag.strip_prefix('<')?.strip_suffix('>')?;
    let bytes = body.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i] != b'=' && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let attr = &body[start..i];
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            let quote = bytes.get(i).copied();
            if quote == Some(b'"') || quote == Some(b'\'') {
                i += 1;
                let vstart = i;
                while i < bytes.len() && bytes[i] != quote.unwrap() {
                    i += 1;
                }
                let value = &body[vstart..i.min(bytes.len())];
                i += 1;
                if attr == name {
                    return Some(value);
                }
            }
        } else if attr == name {
            // Bare attribute: `Some("")` marks presence.
            return Some("");
        }
    }
    None
}

/// Bare-attribute detection (no `=value`).
fn has_bare_attr(tag: &str, name: &str) -> bool {
    let Some(body) = tag.strip_prefix('<').and_then(|b| b.strip_suffix('>')) else {
        return false;
    };
    let bytes = body.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i] != b'=' && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let attr = &body[start..i];
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
            }
        } else if attr == name {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::scan;

    #[test]
    fn plain_sfc_scans_into_one_section() {
        let src = "<script setup lang=\"ts\">\nconst a = ref(0);\n</script>\n<template>\n  <p>{{ a }}</p>\n</template>\n";
        let sections = scan(src).expect("scans");
        assert_eq!(sections.len(), 1);
        let (s, e) = sections[0].script.expect("script range");
        assert_eq!(&src[s..e], "\nconst a = ref(0);\n");
        let (s, e) = sections[0].template;
        assert_eq!(&src[s..e], "\n  <p>{{ a }}</p>\n");
    }

    #[test]
    fn template_only_section_is_admitted() {
        let src = "<template>\n  <p>static</p>\n</template>\n";
        let sections = scan(src).expect("scans");
        assert_eq!(sections.len(), 1);
        assert!(sections[0].script.is_none());
    }

    #[test]
    fn multi_section_modules_scan_in_order() {
        let src = "<script setup>defineOptions({ name: \"A\" });</script>\n<template><p/></template>\n<script setup>defineOptions({ name: \"B\" });</script>\n<template><i/></template>\n";
        let sections = scan(src).expect("scans");
        assert_eq!(sections.len(), 2);
        assert!(sections[0].script.is_some() && sections[1].script.is_some());
    }

    #[test]
    fn non_setup_script_is_rejected() {
        let src = "<script>\nexport default {};\n</script>\n<template><p/></template>\n";
        let diags = scan(src).expect_err("rejected");
        assert!(diags.iter().any(|d| d.message.contains("setup")));
    }

    #[test]
    fn stray_top_level_text_is_rejected() {
        let src = "<template><p/></template>\nstray\n";
        let diags = scan(src).expect_err("rejected");
        assert!(diags.iter().any(|d| d.message.contains("top-level")));
    }
}
