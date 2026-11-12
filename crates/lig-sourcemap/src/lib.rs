//! Chained standard-VLQ source maps (D-11, §9.4): `src→IR` and `IR→Rust`
//! maps with IR node ids in `names[]`, plus a merged consumer map and the
//! NFR-6 verifier. Emitted via oxc's sourcemap fork (standard VLQ, consumable
//! by existing tooling).
//!
//! The IR document is virtual: an IR node's "position" is its ordinal
//! (`(line 0, col = node id)`), and every token carries the node id as its
//! `names[]` entry — a generated-position lookup resolves to `e7`/`d3`-style
//! ids without a bespoke mapping format.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use lig_ir::ViewModule;
use oxc_sourcemap::{SourceMap, SourceMapBuilder};

pub use lig_codegen::{DYNAMIC_BASE, IrLink};

/// One IR → generated-Rust link (the §9.4 link-table entry) — populated since
/// M22: the emitter records the table itself, so consumers never text-scan.
/// Alias of the producer-side [`IrLink`]; the ordinal space it names is
/// defined at [`lig_codegen::DYNAMIC_BASE`].
pub type IrRustLink = IrLink;

/// The typed failure set for the source-map seam (M22) — what failed, the
/// ordinal/offset context, and the fix in words. The [`lig_runtime`]'s
/// `CycleError` pattern: typed data with a module-path code constant and
/// `core::error::Error`, no `miette` — this crate keeps its dependency
/// floor; [`lig_incremental`]'s `EmitError` renders the miette diagnostic
/// (code + help-text) upstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceMapError {
    /// An ordinal the map needs is absent from the producer's link table —
    /// a stale or table-less `GenLinks` reached a map builder.
    LinkMissing {
        /// The IR ordinal the table does not carry.
        ordinal: u32,
    },
    /// A recorded offset does not anchor its own marker in the generated
    /// text — the table and the text disagree (drift, tampering, or a
    /// different generation).
    Anchor {
        /// The IR ordinal whose recorded offset failed the anchor check.
        ordinal: u32,
        /// The recorded byte offset that does not anchor the marker.
        offset: u32,
    },
    /// The merged map failed a serialization round-trip (malformed JSON —
    /// a `oxc_sourcemap` contract break).
    RoundTrip(String),
    /// A verifier resolution failure naming the offending node (`e7`,
    /// `r2`) and what failed, in words.
    Resolve {
        /// The node label the failure is about.
        node: String,
        /// What failed, in words.
        detail: String,
    },
}

impl SourceMapError {
    /// The stable seam code (the `ligature::*` module-path convention —
    /// `EmitError` renders it as the miette code upstream).
    pub const CODE: &'static str = "ligature::sourcemap";
}

impl core::fmt::Display for SourceMapError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::LinkMissing { ordinal } => write!(
                f,
                "{}: IR ordinal {ordinal} is missing from the recorded link table — the \
                 GenLinks came from a producer that does not record offsets; regenerate the unit",
                Self::CODE
            ),
            Self::Anchor { ordinal, offset } => write!(
                f,
                "{}: IR ordinal {ordinal}: recorded offset {offset} does not point at its own \
                 marker in the generated text — regenerate the unit",
                Self::CODE
            ),
            Self::RoundTrip(why) => {
                write!(f, "{}: source-map round-trip failed: {why}", Self::CODE)
            }
            Self::Resolve { node, detail } => {
                write!(f, "{}: {}: {detail}", Self::CODE, node)
            }
        }
    }
}

impl std::error::Error for SourceMapError {}

/// The `names[]` label for an expression node.
#[must_use]
pub fn expr_name(id: u32) -> String {
    format!("e{id}")
}

/// The `names[]` label for a dynamic node.
#[must_use]
pub fn dynamic_name(id: u32) -> String {
    format!("d{id}")
}

/// The `names[]` label for a reaction node.
#[must_use]
pub fn reaction_name(id: u32) -> String {
    format!("r{id}")
}

/// `(line, 0-based col)` of a byte offset in `text`.
#[must_use]
pub fn line_col(offset: u32, text: &str) -> (u32, u32) {
    let end = (offset as usize).min(text.len());
    let mut line = 0u32;
    let mut last_nl = 0usize;
    for (i, b) in text.bytes().take(end).enumerate() {
        if b == b'\n' {
            line += 1;
            last_nl = i + 1;
        }
    }
    (line, (end - last_nl) as u32)
}

/// Build the `src→IR` map (§9.4): every expression span and dynamic span maps
/// its source position to the IR node ordinal. `source` is the original file
/// text (content is embedded for tooling).
#[must_use]
pub fn src_to_ir(module: &ViewModule, source: &str) -> SourceMap<'static> {
    let mut b = SourceMapBuilder::default();
    let source_path = module
        .source_files
        .first()
        .map_or("unknown.tsx", |f| f.path.as_str());
    b.set_file(source_path);
    let src_id = b.add_source_and_content(source_path, source);
    // Tokens: dst = source (line,col), src = the virtual IR document
    // ((0, expr id) for expressions, (1, dynamic idx) for dynamics). Collected
    // first and sorted — VLQ deltas require positional order.
    let mut toks: Vec<(u32, u32, u32, u32, u32)> = Vec::new(); // dst_line, dst_col, src_line, src_col, name
    for (idx, _e) in module.exprs.as_slice().iter().enumerate() {
        let sp = module.exprs.span_of(lig_core::id::ExprId::new(idx as u32));
        if sp.end == 0 {
            continue; // synthesized node without a source position
        }
        let (line, col) = line_col(sp.start, source);
        toks.push((line, col, 0, idx as u32, idx as u32));
    }
    for frag in &module.fragments {
        for (di, d) in frag.dynamics.iter().enumerate() {
            let (start, end) = d.span;
            if end == 0 || end <= start {
                continue;
            }
            let (line, col) = line_col(start, source);
            toks.push((line, col, 1, di as u32, di as u32));
        }
    }
    toks.sort_unstable();
    for (dst_line, dst_col, src_line, src_col, label) in toks {
        let label = if src_line == 0 {
            expr_name(label)
        } else {
            dynamic_name(label)
        };
        let name = b.add_name(leak(label));
        b.add_token(
            dst_line,
            dst_col,
            src_line,
            src_col,
            Some(src_id),
            Some(name),
        );
    }
    b.into_owned_sourcemap().into_inner()
}

/// Build the `IR→Rust` map from the recorded §9.4 link table: reaction and
/// template markers resolve through the emitter-recorded exact offsets (no
/// text scanning), per-expression spans attribute to their owning reaction's
/// recorded region.
///
/// # Errors
/// [`SourceMapError::LinkMissing`] for a table that does not cover the
/// reaction/template names it claims (or the module-anchored staleness
/// guard), [`SourceMapError::Anchor`] for a recorded offset that no longer
/// points at its own marker — producer-side contract breaks, fail-closed.
pub fn ir_to_rust(
    module: &ViewModule,
    generated: &str,
    links: &lig_codegen::GenLinks,
) -> Result<SourceMap<'static>, SourceMapError> {
    let mut b = SourceMapBuilder::default();
    b.set_file("generated.rs");
    // The generated source entry rides the map for the chained consumer
    // (`chain` carries every source entry + content across); its id has no
    // token referencing it — every IR-side token anchors to `ir_id`.
    let _ = b.add_source_and_content("generated.rs", generated);
    let ir_id = b.add_source_and_content("ir", "");

    // Module-anchored staleness guard (M22, fail-closed): the map is
    // module-level (§9.5) — a module that declares reactions must arrive
    // with a recorded table. A table-less `GenLinks` (e.g. the `Default`
    // of a pre-M22 producer) fails here instead of silently emitting an
    // empty IR→Rust map. The reported ordinal is the first claimed reaction
    // id, or 0 for a table-less `Default` (the reaction space starts at 0).
    if links.links.is_empty() && !module.reactions.is_empty() {
        return Err(SourceMapError::LinkMissing {
            ordinal: links.reactions.first().map_or(0, |(rx, _)| *rx),
        });
    }

    // The recorded table, keyed by ordinal (first entry wins — the producer
    // records exactly one entry per marker).
    let recorded = |ordinal: u32| {
        links
            .links
            .iter()
            .find(|l| l.ir_ordinal == ordinal)
            .map(|l| l.generated_offset)
    };
    // The fail-closed anchor check: a recorded offset must still point at its
    // own marker in the generated text (drift/tampering detection — the one
    // text touch left on the consumer side, a `starts_with`, not a scan).
    let anchored = |ordinal: u32, offset: u32, needle: &str| -> Result<u32, SourceMapError> {
        match generated.get(offset as usize..) {
            Some(at) if at.starts_with(needle) => Ok(offset),
            _ => Err(SourceMapError::Anchor { ordinal, offset }),
        }
    };

    // Collect (dst_line, dst_col, src_line, src_col, label-kind, label) then
    // sort — VLQ deltas require positional order on the generated side.
    let mut toks: Vec<(u32, u32, u32, u32, &'static str)> = Vec::new();
    // Reaction markers: exact per-reaction regions, from the recorded table.
    for (rx_id, const_name) in &links.reactions {
        let needle = format!("const {const_name}: u8");
        let off = recorded(*rx_id)
            .ok_or(SourceMapError::LinkMissing { ordinal: *rx_id })
            .and_then(|off| anchored(*rx_id, off, &needle))?;
        let (line, col) = line_col(off, generated);
        toks.push((line, col, 0, *rx_id, leak(reaction_name(*rx_id))));
    }
    // Expression-level links, per-arm (M3): each expression lands at the
    // recorded region of the reaction that owns it. Expressions outside
    // every reaction body fall back to the first reaction's region.
    let owned = expr_owners(module, links);
    for (rx_id, _const_name) in &links.reactions {
        let Some(off) = recorded(*rx_id) else {
            continue; // covered fail-closed by the reaction loop above
        };
        let (line, col) = line_col(off, generated);
        for (idx, owner) in owned.iter().enumerate() {
            if *owner == Some(*rx_id) {
                toks.push((line, col, 0, idx as u32, leak(expr_name(idx as u32))));
            }
        }
    }
    if let Some((first_rx, _name)) = links.reactions.first()
        && let Some(off) = recorded(*first_rx)
    {
        let (line, col) = line_col(off, generated);
        for (idx, owner) in owned.iter().enumerate() {
            if owner.is_none() {
                let sp = module.exprs.span_of(lig_core::id::ExprId::new(idx as u32));
                if sp.end == 0 {
                    continue;
                }
                toks.push((line, col, 0, idx as u32, leak(expr_name(idx as u32))));
            }
        }
    }
    // Template statics, from the recorded table.
    for (tid, static_name) in &links.templates {
        let ordinal = DYNAMIC_BASE + *tid;
        let needle = format!("static {static_name}:");
        let off = recorded(ordinal)
            .ok_or(SourceMapError::LinkMissing { ordinal })
            .and_then(|off| anchored(ordinal, off, &needle))?;
        let (line, col) = line_col(off, generated);
        toks.push((line, col, 0, ordinal, leak(format!("t{tid}"))));
    }
    toks.sort_unstable();
    for (dst_line, dst_col, src_line, src_col, label) in toks {
        let name = b.add_name(label);
        b.add_token(
            dst_line,
            dst_col,
            src_line,
            src_col,
            Some(ir_id),
            Some(name),
        );
    }
    Ok(b.into_owned_sourcemap().into_inner())
}

/// Chain `src→IR` with `IR→Rust` into the merged consumer map (D-11): a
/// generated position resolves directly to the original source position.
#[must_use]
pub fn chain(src_map: &SourceMap<'_>, ir_map: &SourceMap<'_>) -> SourceMap<'static> {
    let mut b = SourceMapBuilder::default();
    b.set_file(ir_map.get_file().unwrap_or("generated.rs"));
    // Carry source entries across.
    let mut src_ids: Vec<Option<u32>> = Vec::new();
    for (i, path) in src_map.get_sources().enumerate() {
        let content = src_map.get_source_content(i as u32).unwrap_or("");
        let id = Some(b.add_source_and_content(leak(path.to_string()), leak(content.to_string())));
        src_ids.push(id);
    }
    // Re-register the IR map's names (the merged builder owns its own table).
    let name_remap: Vec<u32> = (0..ir_map.get_names().len())
        .map(|i| {
            let n = ir_map.get_name(i as u32).unwrap_or("");
            b.add_name(leak(n.to_string()))
        })
        .collect();
    // For every IR→Rust token, resolve its IR ordinal back to the original
    // source via the src→IR map's SOURCE side (src space = the IR doc).
    for token in ir_map.get_tokens() {
        let ir_ordinal = token.get_src_col();
        let ir_line = token.get_src_line();
        let mut best: Option<oxc_sourcemap::Token> = None;
        for t in src_map.get_tokens() {
            if t.get_src_line() == ir_line && t.get_src_col() <= ir_ordinal {
                best = match best {
                    Some(b0) if b0.get_src_col() >= t.get_src_col() => Some(b0),
                    _ => Some(t),
                };
            }
        }
        if let Some(t) = best {
            let sid = src_ids
                .get(t.get_source_id().unwrap_or(0) as usize)
                .copied()
                .flatten();
            let name_id = token
                .get_name_id()
                .map(|nid| name_remap.get(nid as usize).copied().unwrap_or(0));
            b.add_token(
                token.get_dst_line(),
                token.get_dst_col(),
                t.get_src_line(),
                t.get_src_col(),
                sid,
                name_id,
            );
        }
    }
    b.into_owned_sourcemap().into_inner()
}

/// A serialization round-trip: `to_json_string` → `from_json_string`.
///
/// # Errors
/// [`SourceMapError::RoundTrip`] on malformed JSON (cannot round-trip).
pub fn round_trip(map: &SourceMap<'_>) -> Result<SourceMap<'static>, SourceMapError> {
    let json = map.to_json();
    SourceMap::from_json(json).map_err(|e| SourceMapError::RoundTrip(e.to_string()))
}

/// The NFR-6 verifier (§13): every expression span in the module resolves
/// through `src→IR` to its own IR id, every reaction and template link
/// anchors its own marker at its recorded offset.
///
/// # Errors
/// A typed [`SourceMapError`] naming the offending node (`Resolve`),
/// ordinal (`LinkMissing`), or drifted offset (`Anchor`).
pub fn verify(
    module: &ViewModule,
    source: &str,
    generated: &str,
    links: &lig_codegen::GenLinks,
) -> Result<VerifyStats, SourceMapError> {
    let src_map = src_to_ir(module, source);
    let ir_map = ir_to_rust(module, generated, links)?;
    let merged = chain(&src_map, &ir_map);
    let rt = round_trip(&merged)?;

    // 1. Every expression span resolves exactly.
    let mut resolved = 0u32;
    // The lookup table is built once (it was hoisted out of the loop — the
    // per-iteration rebuild was O(n²) on expr count).
    let table = src_map.generate_lookup_table();
    for (idx, _e) in module.exprs.as_slice().iter().enumerate() {
        let sp = module.exprs.span_of(lig_core::id::ExprId::new(idx as u32));
        if sp.end == 0 {
            continue;
        }
        let (line, col) = line_col(sp.start, source);
        let token =
            src_map
                .lookup_token(&table, line, col)
                .ok_or_else(|| SourceMapError::Resolve {
                    node: expr_name(idx as u32),
                    detail: format!("no src→IR token at {line}:{col}"),
                })?;
        let want = expr_name(idx as u32);
        let got = token
            .get_name_id()
            .and_then(|nid| src_map.get_name(nid))
            .unwrap_or("");
        if got != want {
            // The nearest token must still be this expression's own span
            // (spans can share a start); accept only exact attribution.
            return Err(SourceMapError::Resolve {
                node: want,
                detail: format!("resolved to `{got}`"),
            });
        }
        resolved += 1;
    }
    // 2. Every recorded link still anchors its own marker in the generated
    // text — the fail-closed anchor contract (M22), re-checked at the
    // verifier: reaction markers AND template statics, exact offsets, no
    // scans.
    for (rx, name) in &links.reactions {
        let needle = format!("const {name}: u8");
        let off = links
            .links
            .iter()
            .find(|l| l.ir_ordinal == *rx)
            .map(|l| l.generated_offset)
            .ok_or(SourceMapError::LinkMissing { ordinal: *rx })?;
        let at = generated.get(off as usize..).unwrap_or("");
        if !at.starts_with(&needle) {
            return Err(SourceMapError::Anchor {
                ordinal: *rx,
                offset: off,
            });
        }
    }
    for (tid, name) in &links.templates {
        let ordinal = DYNAMIC_BASE + *tid;
        let needle = format!("static {name}:");
        let off = links
            .links
            .iter()
            .find(|l| l.ir_ordinal == ordinal)
            .map(|l| l.generated_offset)
            .ok_or(SourceMapError::LinkMissing { ordinal })?;
        let at = generated.get(off as usize..).unwrap_or("");
        if !at.starts_with(&needle) {
            return Err(SourceMapError::Anchor {
                ordinal,
                offset: off,
            });
        }
    }
    Ok(VerifyStats {
        expr_spans_resolved: resolved,
        reactions_linked: links.reactions.len() as u32,
        merged_tokens: rt.get_tokens().len() as u32,
    })
}

/// Verifier statistics (surfaced by `lig build` / the CI gates).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifyStats {
    /// Expression spans resolved exactly through `src→IR`.
    pub expr_spans_resolved: u32,
    /// Reaction markers linked into the generated text.
    pub reactions_linked: u32,
    /// Token count of the merged map after a serialization round-trip.
    pub merged_tokens: u32,
}

/// Per-expression reaction ownership for per-arm attribution (M3): an entry
/// owned by the reaction whose body it is (exact), else by the reaction with
/// the smallest body span containing it; `None` when no body covers it.
#[must_use]
fn expr_owners(module: &ViewModule, links: &lig_codegen::GenLinks) -> Vec<Option<u32>> {
    let mut owners: Vec<Option<u32>> = vec![None; module.exprs.len()];
    let bodies: Vec<(u32, (u32, u32))> = links
        .reactions
        .iter()
        .filter_map(|(rx, _)| {
            module.reactions.get(*rx as usize).map(|d| {
                let sp = module.exprs.span_of(d.body);
                (*rx, (sp.start, sp.end))
            })
        })
        .collect();
    for (idx, _e) in module.exprs.as_slice().iter().enumerate() {
        let sp = module.exprs.span_of(lig_core::id::ExprId::new(idx as u32));
        if sp.end == 0 {
            continue;
        }
        let mut best: Option<(u32, u32)> = None; // (rx id, containing width)
        for (rx, (b0, b1)) in &bodies {
            if *b1 <= *b0 {
                continue; // synthesized body — no containment geometry
            }
            let exact = sp.start == *b0 && sp.end == *b1;
            let contains = sp.start >= *b0 && sp.end <= *b1;
            if !exact && !contains {
                continue;
            }
            let width = b1.saturating_sub(*b0);
            let better = match best {
                None => true,
                Some((_rx, w)) => width < w,
            };
            if better {
                best = Some((*rx, width));
            }
        }
        owners[idx] = best.map(|(rx, _)| rx);
    }
    // Exact body entries always win over containment (same span would tie).
    for (rx, _const) in &links.reactions {
        if let Some(d) = module.reactions.get(*rx as usize) {
            let b = d.body.get() as usize;
            if b < owners.len() {
                owners[b] = Some(*rx);
            }
        }
    }
    owners
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

#[cfg(test)]
mod tests {
    use super::{chain, expr_name, line_col, round_trip, src_to_ir, verify};
    use lig_codegen::generate_component;
    use lig_core::id::ComponentId;

    fn counter() -> (lig_ir::ViewModule, String) {
        let module = lig_parse_solid::lower_module(
            lig_parse_solid::tests_support::COUNTER_TSX,
            "counter.tsx",
        )
        .expect("lower");
        (module, COUNTER_SRC.to_string())
    }

    const COUNTER_SRC: &str = lig_parse_solid::tests_support::COUNTER_TSX;

    #[test]
    fn src_to_ir_maps_every_expr_span_exactly() {
        let (m, src) = counter();
        let map = src_to_ir(&m, &src);
        let table = map.generate_lookup_table();
        for (idx, _e) in m.exprs.as_slice().iter().enumerate() {
            let sp = m.exprs.span_of(lig_core::id::ExprId::new(idx as u32));
            if sp.end == 0 {
                continue;
            }
            let (line, col) = line_col(sp.start, &src);
            let t = map
                .lookup_token(&table, line, col)
                .unwrap_or_else(|| panic!("e{idx} unresolved at {line}:{col}"));
            let got = t
                .get_name_id()
                .and_then(|nid| map.get_name(nid))
                .unwrap_or("");
            assert_eq!(got, expr_name(idx as u32).as_str(), "e{idx}");
        }
    }

    #[test]
    fn maps_round_trip_through_standard_json() {
        let (m, src) = counter();
        let map = src_to_ir(&m, &src);
        let rt = round_trip(&map).expect("round-trips");
        assert_eq!(rt.get_tokens().len(), map.get_tokens().len());
        assert!(map.to_json_string().contains("\"version\":3"));
    }

    #[test]
    fn verifier_resolves_all_spans_and_reactions() {
        let (m, src) = counter();
        let unit = generate_component(&m, ComponentId::new(0)).expect("codegen");
        let stats = verify(&m, &src, &unit.text, &unit.links).expect("verify");
        assert!(stats.expr_spans_resolved > 0);
        assert!(stats.reactions_linked >= 4, "counter reactions");
        assert!(stats.merged_tokens > 0);
    }

    #[test]
    fn expressions_map_per_arm_to_their_own_reaction_region() {
        let (m, _src) = counter();
        let unit = generate_component(&m, ComponentId::new(0)).expect("codegen");
        let ir_map = super::ir_to_rust(&m, &unit.text, &unit.links).expect("ir map");
        // The generated position of each reaction's const marker.
        let region_of = |name: &str| -> (u32, u32) {
            let off = unit
                .text
                .find(&format!("const {name}: u8"))
                .expect("marker");
            super::line_col(off as u32, &unit.text)
        };
        let regions: Vec<(u32, (u32, u32))> = unit
            .links
            .reactions
            .iter()
            .map(|(rx, name)| (*rx, region_of(name)))
            .collect();
        assert!(regions.len() >= 2, "counter has several arms");
        // Every expression attributed to a reaction resolves to that
        // reaction's own region — different arms map to different positions.
        let mut seen: Vec<(u32, (u32, u32))> = Vec::new();
        for token in ir_map.get_tokens() {
            let name = token
                .get_name_id()
                .and_then(|nid| ir_map.get_name(nid))
                .unwrap_or("");
            let Some(id) = name
                .strip_prefix('e')
                .and_then(|rest| rest.parse::<u32>().ok())
            else {
                continue;
            };
            let owner = super::expr_owners(&m, &unit.links)[id as usize];
            if let Some(rx) = owner {
                let want = regions.iter().find(|(r, _)| *r == rx).expect("region").1;
                let got = (token.get_dst_line(), token.get_dst_col());
                assert_eq!(got, want, "e{id} must land in r{rx}'s own region");
                if !seen.iter().any(|(r, _)| *r == rx) {
                    seen.push((rx, got));
                }
            }
        }
        assert!(
            seen.len() >= 2,
            "at least two arms carry their own expressions: {seen:?}"
        );
    }

    #[test]
    fn merged_map_chains_generated_positions_to_source() {
        let (m, src) = counter();
        let unit = generate_component(&m, ComponentId::new(0)).expect("codegen");
        let src_map = src_to_ir(&m, &src);
        let ir_map = super::ir_to_rust(&m, &unit.text, &unit.links).expect("ir map");
        let merged = chain(&src_map, &ir_map);
        assert!(merged.get_tokens().len() > 0, "merged map carries links");
    }

    /// M22: the consumer reads the recorded §9.4 table — every reaction and
    /// template token sits exactly at its recorded offset (no scanning).
    #[test]
    fn ir_to_rust_lands_every_recorded_link_at_its_exact_offset() {
        let (m, _src) = counter();
        let unit = generate_component(&m, ComponentId::new(0)).expect("codegen");
        let ir_map = super::ir_to_rust(&m, &unit.text, &unit.links).expect("ir map");
        for link in &unit.links.links {
            let (line, col) = super::line_col(link.generated_offset, &unit.text);
            let want = if link.ir_ordinal >= super::DYNAMIC_BASE {
                format!("t{}", link.ir_ordinal - super::DYNAMIC_BASE)
            } else {
                super::reaction_name(link.ir_ordinal)
            };
            let hit = ir_map.get_tokens().into_iter().any(|t| {
                t.get_dst_line() == line
                    && t.get_dst_col() == col
                    && t.get_name_id()
                        .and_then(|nid| ir_map.get_name(nid))
                        .is_some_and(|n| n == want)
            });
            assert!(
                hit,
                "link ordinal {} must land at its recorded offset ({line},{col}) as {want}",
                link.ir_ordinal
            );
        }
    }

    /// M22 fail-closed: a `GenLinks` without a recorded table (e.g. the
    /// `Default` a stale producer hands over) must fail, not silently
    /// produce an empty map.
    #[test]
    fn ir_to_rust_is_fail_closed_on_an_unrecorded_table() {
        let (m, _src) = counter();
        let unit = generate_component(&m, ComponentId::new(0)).expect("codegen");
        let stale = lig_codegen::GenLinks::default();
        let err = super::ir_to_rust(&m, &unit.text, &stale)
            .expect_err("a table-less GenLinks must fail closed");
        assert!(
            matches!(err, super::SourceMapError::LinkMissing { .. }),
            "the failure names the seam: {err}"
        );
    }

    /// M22 fail-closed: a recorded offset that no longer anchors its own
    /// marker (drift/tampering) must fail, not mis-attribute positions.
    #[test]
    fn ir_to_rust_is_fail_closed_on_a_tampered_offset() {
        let (m, _src) = counter();
        let unit = generate_component(&m, ComponentId::new(0)).expect("codegen");
        let mut drifted = unit.links.clone();
        let l0 = drifted
            .links
            .first_mut()
            .expect("the counter records links");
        l0.generated_offset += 7; // off the marker's head
        let err = super::ir_to_rust(&m, &unit.text, &drifted)
            .expect_err("a drifting offset must fail closed");
        assert!(
            matches!(err, super::SourceMapError::Anchor { .. }),
            "the failure names the drift: {err}"
        );
    }

    /// M22: `verify` re-checks every recorded offset against the generated
    /// text — the fail-closed anchor contract holds at the verifier too.
    #[test]
    fn verify_checks_recorded_offsets_anchoring() {
        let (m, src) = counter();
        let unit = generate_component(&m, ComponentId::new(0)).expect("codegen");
        let ok = verify(&m, &src, &unit.text, &unit.links).expect("verify");
        assert!(ok.reactions_linked >= 4);
        let mut drifted = unit.links.clone();
        let last = drifted.links.last_mut().expect("recorded links");
        last.generated_offset += 3;
        let err = verify(&m, &src, &unit.text, &drifted)
            .expect_err("verify must catch a drifting anchor");
        assert!(
            matches!(err, super::SourceMapError::Anchor { .. }),
            "the verifier failure names the drift: {err}"
        );
    }

    /// M22 (typed failures): `ir_to_rust` reports a stale table as the typed
    /// [`SourceMapError::LinkMissing`] carrying the offending ordinal, and
    /// a drifting table as [`SourceMapError::Anchor`] carrying ordinal plus
    /// offset — the CycleError pattern (typed data, module-path code).
    #[test]
    fn failures_are_typed_with_ordinal_and_offset_context() {
        let (m, _src) = counter();
        let unit = generate_component(&m, ComponentId::new(0)).expect("codegen");

        // Stale (table-less) links → LinkMissing with the module's first
        // reaction ordinal.
        let err = super::ir_to_rust(&m, &unit.text, &lig_codegen::GenLinks::default())
            .expect_err("stale links fail closed");
        match &err {
            super::SourceMapError::LinkMissing { ordinal } => {
                assert_eq!(
                    *ordinal, unit.links.reactions[0].0,
                    "the first reaction ordinal"
                );
            }
            other => panic!("want LinkMissing, got {other:?}"),
        }
        assert!(
            err.to_string()
                .starts_with(&format!("{}:", super::SourceMapError::CODE)),
            "the Display heads with the seam code: {err}"
        );

        // A drifting offset → Anchor with ordinal and offset.
        let mut drifted = unit.links.clone();
        let l0 = drifted.links.first_mut().expect("recorded links");
        l0.generated_offset += 7;
        let err = super::ir_to_rust(&m, &unit.text, &drifted)
            .expect_err("a drifting offset fails closed");
        match &err {
            super::SourceMapError::Anchor { ordinal, offset } => {
                assert_eq!(*ordinal, unit.links.links[0].ir_ordinal);
                assert_eq!(*offset, unit.links.links[0].generated_offset + 7);
            }
            other => panic!("want Anchor, got {other:?}"),
        }

        // The verifier's resolution failures carry the node label.
        let err = super::SourceMapError::Resolve {
            node: "e3".into(),
            detail: "no src→IR token at 4:12".into(),
        };
        assert!(err.to_string().contains("e3"), "{err}");

        // Round-trip failures surface as the typed variant.
        let err = super::SourceMapError::RoundTrip("boom".into());
        assert!(err.to_string().contains("boom"), "{err}");
        // The typed error implements `core::error::Error` (object-safe check).
        let err: Box<dyn std::error::Error> = Box::new(err);
        assert!(err.to_string().contains("boom"));
    }
}
