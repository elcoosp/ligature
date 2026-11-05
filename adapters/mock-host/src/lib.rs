//! Mock Host adapter (§4.2 `adapters/`): records the commit stream for spike
//! S1 render tests, compile-clean gates, and benchmarks. Hosts mirror state —
//! this one mirrors into a queryable log.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use lig_runtime::host::{
    AttrVal, EventKind, HandlerId, HookId, Host, NodeId, PropVal, StyleConstId, TemplateDesc,
};
use std::fmt::Write as _;

/// One commit in the stream.
#[derive(Debug, Clone, PartialEq)]
pub enum Commit {
    /// `create` — tag, attrs, style const.
    Create {
        /// The node.
        node: NodeId,
        /// Tag name.
        tag: String,
        /// Static attrs.
        attrs: Vec<(String, String)>,
        /// Static style id.
        style: Option<u32>,
    },
    /// `destroy`.
    Destroy {
        /// The node.
        node: NodeId,
    },
    /// `set_text`.
    SetText {
        /// The node.
        node: NodeId,
        /// New text.
        text: String,
    },
    /// `set_attr`.
    SetAttr {
        /// The node.
        node: NodeId,
        /// Attribute key.
        key: String,
        /// New value (None = cleared).
        val: Option<String>,
    },
    /// `set_class`.
    SetClass {
        /// The node.
        node: NodeId,
        /// New class (None = cleared).
        class: Option<String>,
    },
    /// `set_style`.
    SetStyle {
        /// The node.
        node: NodeId,
        /// Style const id.
        style: u32,
    },
    /// `set_style_dyn`.
    SetStyleDyn {
        /// The node.
        node: NodeId,
        /// Number of props.
        count: usize,
    },
    /// `set_html` (tiered dynamic, §6.2).
    SetHtml {
        /// The node.
        node: NodeId,
        /// Raw HTML.
        html: String,
    },
    /// `insert`.
    Insert {
        /// Parent.
        parent: NodeId,
        /// Child.
        child: NodeId,
        /// Index.
        index: u32,
    },
    /// `remove`.
    Remove {
        /// Parent.
        parent: NodeId,
        /// Child.
        child: NodeId,
    },
    /// `move_`.
    Move {
        /// Parent.
        parent: NodeId,
        /// Child.
        child: NodeId,
        /// Index.
        index: u32,
    },
    /// `attach`.
    Attach {
        /// The node.
        node: NodeId,
        /// Event kind.
        ev: &'static str,
        /// Handler id.
        handler: u32,
        /// Delegation flag.
        delegate: bool,
    },
    /// `detach`.
    Detach {
        /// The node.
        node: NodeId,
        /// Event kind.
        ev: &'static str,
        /// Handler id.
        handler: u32,
    },
    /// `on_mount`.
    OnMount {
        /// The node.
        node: NodeId,
        /// Hook id.
        hook: u32,
    },
    /// `on_unmount`.
    OnUnmount {
        /// The node.
        node: NodeId,
        /// Hook id.
        hook: u32,
    },
}

/// The mock host: a commit log plus the bound style table.
#[derive(Debug, Default)]
pub struct MockHost {
    /// The recorded commit stream.
    pub commits: Vec<Commit>,
    styles: Vec<String>,
}

impl MockHost {
    /// A fresh host with an empty style table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One-time `bind_style_table` (§10.1): hosts pull consts by id.
    pub fn bind_style_table(&mut self, css: &[&str]) {
        self.styles = css.iter().map(|s| (*s).to_string()).collect();
    }

    /// The bound style css for an id.
    #[must_use]
    pub fn style_css(&self, id: u32) -> &str {
        self.styles.get(id as usize).map_or("", String::as_str)
    }

    /// The latest text set on a node.
    #[must_use]
    pub fn text_of(&self, node: NodeId) -> Option<&str> {
        self.commits.iter().rev().find_map(|c| match c {
            Commit::SetText { node: n, text } if n == &node => Some(text.as_str()),
            _ => None,
        })
    }

    /// The latest class set on a node.
    #[must_use]
    pub fn class_of(&self, node: NodeId) -> Option<Option<&str>> {
        self.commits.iter().rev().find_map(|c| match c {
            Commit::SetClass { node: n, class } if n == &node => Some(class.as_deref()),
            _ => None,
        })
    }

    /// The most recent SetText commit in the stream.
    #[must_use]
    pub fn last_text(&self) -> Option<&str> {
        self.commits.iter().rev().find_map(|c| match c {
            Commit::SetText { text, .. } => Some(text.as_str()),
            _ => None,
        })
    }

    /// The most recent tag created (last Create commit).
    #[must_use]
    pub fn last_created_tag(&self) -> Option<&str> {
        self.commits.iter().rev().find_map(|c| match c {
            Commit::Create { tag, .. } => Some(tag.as_str()),
            _ => None,
        })
    }

    /// The latest raw HTML set on a node.
    #[must_use]
    pub fn html_of(&self, node: NodeId) -> Option<&str> {
        self.commits.iter().rev().find_map(|c| match c {
            Commit::SetHtml { node: n, html } if n == &node => Some(html.as_str()),
            _ => None,
        })
    }

    /// Count of commits of a given kind name (`"Create"`, `"SetText"`, …).
    #[must_use]
    pub fn count_kind(&self, kind: &str) -> usize {
        self.commits.iter().filter(|c| kind_name(c) == kind).count()
    }

    /// Debug rendering of the whole stream.
    #[must_use]
    pub fn dump(&self) -> String {
        let mut out = String::new();
        for c in &self.commits {
            let _ = writeln!(out, "{c:?}");
        }
        out
    }
}

fn kind_name(c: &Commit) -> &'static str {
    match c {
        Commit::Create { .. } => "Create",
        Commit::Destroy { .. } => "Destroy",
        Commit::SetText { .. } => "SetText",
        Commit::SetAttr { .. } => "SetAttr",
        Commit::SetClass { .. } => "SetClass",
        Commit::SetStyle { .. } => "SetStyle",
        Commit::SetStyleDyn { .. } => "SetStyleDyn",
        Commit::SetHtml { .. } => "SetHtml",
        Commit::Insert { .. } => "Insert",
        Commit::Remove { .. } => "Remove",
        Commit::Move { .. } => "Move",
        Commit::Attach { .. } => "Attach",
        Commit::Detach { .. } => "Detach",
        Commit::OnMount { .. } => "OnMount",
        Commit::OnUnmount { .. } => "OnUnmount",
    }
}

fn attr_to_string(v: &AttrVal<'_>) -> String {
    match v {
        AttrVal::Str(s) => (*s).to_string(),
        AttrVal::Bool(b) => (*b).to_string(),
        AttrVal::Num(n) => (*n).to_string(),
    }
}

impl Host for MockHost {
    fn create(&mut self, node: NodeId, desc: TemplateDesc<'_>) {
        self.commits.push(Commit::Create {
            node,
            tag: desc.tag.to_string(),
            attrs: desc
                .attrs
                .iter()
                .map(|(k, v)| ((*k).to_string(), attr_to_string(v)))
                .collect(),
            style: desc.style.map(StyleConstId::get),
        });
    }

    fn destroy(&mut self, node: NodeId) {
        self.commits.push(Commit::Destroy { node });
    }

    fn set_text(&mut self, node: NodeId, text: &str) {
        self.commits.push(Commit::SetText {
            node,
            text: text.to_string(),
        });
    }

    fn set_attr(&mut self, node: NodeId, key: &str, val: Option<AttrVal<'_>>) {
        self.commits.push(Commit::SetAttr {
            node,
            key: key.to_string(),
            val: val.map(|v| attr_to_string(&v)),
        });
    }

    fn set_class(&mut self, node: NodeId, class: Option<&str>) {
        self.commits.push(Commit::SetClass {
            node,
            class: class.map(str::to_string),
        });
    }

    fn set_style(&mut self, node: NodeId, style: StyleConstId) {
        self.commits.push(Commit::SetStyle {
            node,
            style: style.get(),
        });
    }

    fn set_style_dyn(&mut self, node: NodeId, props: &[(u32, PropVal<'_>)]) {
        self.commits.push(Commit::SetStyleDyn {
            node,
            count: props.len(),
        });
    }

    fn set_html(&mut self, node: NodeId, html: &str) {
        self.commits.push(Commit::SetHtml {
            node,
            html: html.to_string(),
        });
    }

    fn insert(&mut self, parent: NodeId, child: NodeId, index: u32) {
        self.commits.push(Commit::Insert {
            parent,
            child,
            index,
        });
    }

    fn remove(&mut self, parent: NodeId, child: NodeId) {
        self.commits.push(Commit::Remove { parent, child });
    }

    fn move_(&mut self, parent: NodeId, child: NodeId, index: u32) {
        self.commits.push(Commit::Move {
            parent,
            child,
            index,
        });
    }

    fn attach(&mut self, node: NodeId, ev: EventKind, handler: HandlerId, delegate: bool) {
        self.commits.push(Commit::Attach {
            node,
            ev: ev.as_str(),
            handler: handler.get(),
            delegate,
        });
    }

    fn detach(&mut self, node: NodeId, ev: EventKind, handler: HandlerId) {
        self.commits.push(Commit::Detach {
            node,
            ev: ev.as_str(),
            handler: handler.get(),
        });
    }

    fn on_mount(&mut self, node: NodeId, hook: HookId) {
        self.commits.push(Commit::OnMount {
            node,
            hook: hook.get(),
        });
    }

    fn on_unmount(&mut self, node: NodeId, hook: HookId) {
        self.commits.push(Commit::OnUnmount {
            node,
            hook: hook.get(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::MockHost;
    use lig_runtime::host::{AttrVal, EventKind, HandlerId, Host, NodeId, TemplateDesc};

    #[test]
    fn records_the_commit_stream() {
        let mut h = MockHost::new();
        h.create(
            NodeId(0),
            TemplateDesc {
                tag: "div",
                attrs: &[],
                style: None,
            },
        );
        h.set_text(NodeId(0), "hi");
        h.attach(NodeId(0), EventKind::Click, HandlerId(1), true);
        assert_eq!(h.count_kind("Create"), 1);
        assert_eq!(h.text_of(NodeId(0)), Some("hi"));
        assert_eq!(h.last_created_tag(), Some("div"));
    }

    #[test]
    fn style_table_binding() {
        let mut h = MockHost::new();
        h.bind_style_table(&["display:flex", "padding:16px"]);
        assert_eq!(h.style_css(1), "padding:16px");
        let _ = AttrVal::Bool(true);
    }
}
