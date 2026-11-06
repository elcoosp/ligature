//! Published, semver-stable runtime core (D-4/D-5, §10): the node slab with
//! `Copy` handles, the object-safe `Host` commit stream, the const-schedule
//! flush loop, keyed LIS reconciliation, and event dispatch.
//!
//! `#![no_std]` + `alloc`; `forbid(unsafe_code)` — the sole unsafe exception
//! in the project lives in `lig-patch`, never here (§15). Hosts keep their
//! native representation side-tabled by `NodeId`; the slab only tracks
//! structure and lifecycle order.

#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

extern crate alloc;

// Test harnesses run on the host; std is test-only.
#[cfg(test)]
extern crate std;

pub mod dirty;
pub mod fmt;
pub mod fragment;
pub mod host;
pub mod pure;
pub mod reconcile;
pub mod schedule;
pub mod template;

pub use dirty::DirtySet;
pub use dirty::REACTION_LIMIT;
pub use fmt::format_i64;
pub use fragment::{BranchInstance, Fragment, ListItem, ListSlot, NodeKind, NodeRecord};
pub use host::{
    AttrVal, EvData, EventKind, HandlerId, HookId, Host, NodeId, PropVal, StyleConstId,
    TemplateDesc,
};
pub use reconcile::{ListOp, SeqOp, key_hash, keyed_diff, lis_sequence, plan_keyed, unkeyed_diff};
pub use schedule::{MAX_PASSES, Reactions, Schedule, flush};
pub use template::{AnchorStep, Template, TplAttr, TplNode};

/// The `lig-runtime` version — published independently of the compiler
/// (§10 posture). Semver-stable contract surface.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Prelude re-exports for generated code (`use lig_runtime::prelude::*;`).
pub mod prelude {
    pub use super::{
        AttrVal, DirtySet, EvData, EventKind, Fragment, HandlerId, Host, MAX_PASSES, NodeId,
        PropVal, Reactions, Schedule, StyleConstId, Template, TemplateDesc, TplAttr, TplNode,
        flush,
    };
}
