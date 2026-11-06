//! The module root (§5.1) plus reaction declarations (§7.1) and span table.

use crate::expr::{Expr, ExprTable};
use crate::fragment::{ComponentDecl, FragmentIr, Import, StateDecl};
use crate::style::{ShapeTable, StyleConst};
use crate::template::TemplateIr;
use crate::{ComponentId, ExprId, FileId, FragmentId, Frontend, StrId, StyleConstId, TemplateId};
use lig_core::id::ReactionId;
use lig_core::intern::StringTable;
use serde::{Deserialize, Serialize};

/// The View IR schema version (§5.6: bump on any field change). v4 added
/// the `lints` channel, `Expr::Event/Array/Native`, slot fills, and the
/// per-expr/dynamic span table (admission + src→IR maps, M1). v5 gives the
/// slot outlet its own `fallback` fragment (M6 — named slots and `<slot>`
/// fallback content end to end).
pub const VIEW_IR_VERSION: u16 = 5;

/// A compiled view module — the only serialized artifact (§5.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewModule {
    /// `view_ir_version`.
    pub version: u16,
    /// Producing frontend; the only parity-ignored field (§6.4).
    pub frontend: Frontend,
    /// Source files with content hashes.
    pub source_files: Vec<SourceFile>,
    /// Interned strings; index-stable, sorted on serialize.
    pub strings: StringTable,
    /// Component declarations (roots first).
    pub components: Vec<ComponentDecl>,
    /// Deduped, content-addressed templates.
    pub templates: Vec<TemplateIr>,
    /// `[0..k]` = component roots in declaration order, then branch/for/slot
    /// bodies in discovery order (canonicalized: components sorted by name,
    /// depth-first).
    pub fragments: Vec<FragmentIr>,
    /// Typed expression forest (§5.5).
    pub exprs: ExprTable,
    /// Signals, memos, props, locals.
    pub states: Vec<StateDecl>,
    /// Memos, effects, binds (§7).
    pub reactions: Vec<ReactionDecl>,
    /// Interned static styles.
    pub styles: Vec<StyleConst>,
    /// Registered structural shapes.
    pub shapes: ShapeTable,
    /// Whitelisted pure builtins and model fns (§5.5 `Call`).
    pub pure_fns: Vec<crate::expr::PureFnDecl>,
    /// Cross-module imports.
    pub imports: Vec<Import>,
    /// Front-end Lint notes (§6.5 Lint tier): lowered constructs that carry a
    /// performance/idiom flag. Admission re-renders these as `LIG-W`
    /// diagnostics; canonical JSON carries them so both frontends' lints are
    /// parity-visible.
    pub lints: Vec<LintNote>,
    /// Source spans, `(file, start, end)` byte ranges (§5.1).
    pub spans: SpanTable,
}

/// One Lint note attached at lowering time (§6.5 Lint tier).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintNote {
    /// Stable `LIG-W###` code.
    pub code: String,
    /// One-line message.
    pub message: String,
    /// Byte range in the source file.
    pub start: u32,
    /// Byte range end.
    pub end: u32,
}

/// A source file entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFile {
    /// File id used by the span table.
    pub id: FileId,
    /// Path (relative to the module root).
    pub path: String,
    /// FNV-1a 64 of the content.
    pub hash: u64,
}

/// A span-table entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanEntry {
    /// Owning file.
    pub file: FileId,
    /// Start byte.
    pub start: u32,
    /// End byte.
    pub end: u32,
}

/// The span table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanTable {
    /// Entries.
    pub entries: Vec<SpanEntry>,
}

impl SpanTable {
    /// Push a span, returning its id.
    pub fn push(&mut self, file: FileId, start: u32, end: u32) -> lig_core::id::SpanId {
        let id =
            lig_core::id::SpanId::new(self.entries.len().try_into().expect("span table overflow"));
        self.entries.push(SpanEntry { file, start, end });
        id
    }
}

/// Effect tiers (C8 — Solid's computed / render-effect / user-effect).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// Memos/computed — run before renders.
    Pre,
    /// Render effects (binds).
    Render,
    /// User effects.
    Post,
}

/// A reaction declaration (§7.1): the graph replaced by two const arrays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReactionDecl {
    /// Kind.
    pub kind: ReactionKind,
    /// State fields read (from Expr dataflow).
    pub reads: Vec<StrId>,
    /// State fields written (Effect/Memo only).
    pub writes: Vec<StrId>,
    /// The body expression.
    pub body: ExprId,
    /// Owning fragment.
    pub owner: FragmentId,
}

/// Reaction kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReactionKind {
    /// Memo — computes a state field; scheduled before readers (§7.1).
    Memo {
        /// Target state field.
        target: StrId,
    },
    /// User effect.
    Effect {
        /// Execution tier.
        tier: Tier,
    },
    /// Per-dynamic update function (text/attr/style/prop).
    Bind {
        /// Index into the owning fragment's `dynamics`.
        dynamic: u32,
    },
}

impl ViewModule {
    /// A new, empty module for `frontend`.
    #[must_use]
    pub fn new(frontend: Frontend) -> Self {
        Self {
            version: VIEW_IR_VERSION,
            frontend,
            source_files: Vec::new(),
            strings: StringTable::new(),
            components: Vec::new(),
            templates: Vec::new(),
            fragments: Vec::new(),
            exprs: ExprTable::new(),
            states: Vec::new(),
            reactions: Vec::new(),
            styles: Vec::new(),
            shapes: ShapeTable::default(),
            pure_fns: Vec::new(),
            imports: Vec::new(),
            lints: Vec::new(),
            spans: SpanTable::default(),
        }
    }

    /// Intern a template, deduping by content hash (C1). Returns the
    /// template id (existing if already present).
    pub fn intern_template(&mut self, tpl: TemplateIr) -> TemplateId {
        if let Some(pos) = self.templates.iter().position(|t| t.hash == tpl.hash) {
            return TemplateId::new(pos as u32);
        }
        let id = TemplateId::new(self.templates.len().try_into().expect("template overflow"));
        self.templates.push(tpl);
        id
    }

    /// Intern a static style, deduping by content hash (§8.1).
    pub fn intern_style(&mut self, style: StyleConst) -> StyleConstId {
        if let Some(pos) = self.styles.iter().position(|s| s.hash == style.hash) {
            return StyleConstId::new(pos as u32);
        }
        let id = StyleConstId::new(self.styles.len().try_into().expect("style overflow"));
        self.styles.push(style);
        id
    }

    /// Push a fragment, returning its id.
    pub fn push_fragment(&mut self, fragment: FragmentIr) -> FragmentId {
        let id = FragmentId::new(self.fragments.len().try_into().expect("fragment overflow"));
        self.fragments.push(fragment);
        id
    }

    /// Push a reaction, returning its id.
    pub fn push_reaction(&mut self, reaction: ReactionDecl) -> ReactionId {
        let id = ReactionId::new(self.reactions.len().try_into().expect("reaction overflow"));
        self.reactions.push(reaction);
        id
    }

    /// Insert an expression, returning its id.
    pub fn insert_expr(&mut self, expr: Expr) -> ExprId {
        self.exprs.insert(expr)
    }

    /// Intern a pure fn or model fn by name, deduping (§5.5).
    pub fn intern_pure_fn(
        &mut self,
        name: StrId,
        kind: crate::expr::PureFnKind,
    ) -> lig_core::id::PureFnId {
        if let Some(pos) = self.pure_fns.iter().position(|f| f.name == name) {
            return lig_core::id::PureFnId::new(pos as u32);
        }
        let id =
            lig_core::id::PureFnId::new(self.pure_fns.len().try_into().expect("pure fn overflow"));
        self.pure_fns.push(crate::expr::PureFnDecl { name, kind });
        id
    }

    /// Resolve a state field name to its declaration.
    #[must_use]
    pub fn state_by_name(&self, name: StrId) -> Option<&StateDecl> {
        self.states.iter().find(|s| s.name == name)
    }

    /// The component root fragment for a component id.
    #[must_use]
    pub fn component_root(&self, c: ComponentId) -> Option<FragmentId> {
        self.components.get(c.get() as usize).map(|d| d.root)
    }
}

#[cfg(test)]
mod tests {
    use super::{ReactionDecl, ReactionKind, Tier, VIEW_IR_VERSION, ViewModule};
    use crate::fragment::{ComponentDecl, FragmentIr, StateDecl, StateKind};
    use crate::{ExprId, FragmentId, Frontend, StrId, TemplateId, Ty};

    #[test]
    fn new_module_has_current_version() {
        let m = ViewModule::new(Frontend::Solid);
        assert_eq!(m.version, VIEW_IR_VERSION);
        assert_eq!(m.version, 5);
    }

    #[test]
    fn template_interning_dedups_by_hash() {
        use crate::template::{NodeIr, TemplateIr};
        let mut m = ViewModule::new(Frontend::Solid);
        let t1 = m.intern_template(TemplateIr::new(NodeIr::Marker));
        let t2 = m.intern_template(TemplateIr::new(NodeIr::Marker));
        assert_eq!(t1, t2);
        assert_eq!(m.templates.len(), 1);
    }

    #[test]
    fn reactions_roundtrip_with_tiers() {
        let mut m = ViewModule::new(Frontend::Vue);
        m.fragments.push(FragmentIr {
            name: StrId::new(0),
            template: TemplateId::new(0),
            dynamics: vec![],
            children: vec![],
            props: vec![],
            returns: vec![],
        });
        let id = m.push_reaction(ReactionDecl {
            kind: ReactionKind::Effect { tier: Tier::Post },
            reads: vec![StrId::new(1)],
            writes: vec![],
            body: ExprId::new(0),
            owner: FragmentId::new(0),
        });
        assert_eq!(id, lig_core::id::ReactionId::new(0));
        assert!(matches!(
            m.reactions[0].kind,
            ReactionKind::Effect { tier: Tier::Post }
        ));
    }

    #[test]
    fn state_lookup_by_name() {
        let mut m = ViewModule::new(Frontend::Solid);
        m.fragments.push(FragmentIr {
            name: StrId::new(0),
            template: TemplateId::new(0),
            dynamics: vec![],
            children: vec![],
            props: vec![],
            returns: vec![],
        });
        m.components.push(ComponentDecl {
            name: StrId::new(0),
            root: FragmentId::new(0),
            props: vec![],
        });
        let count = m.strings.intern("count");
        m.states.push(StateDecl {
            name: count,
            owner: FragmentId::new(0),
            kind: StateKind::Signal,
            ty: Ty::Int,
            init: None,
        });
        assert!(m.state_by_name(count).is_some());
        assert!(m.state_by_name(StrId::new(99)).is_none());
    }
}
