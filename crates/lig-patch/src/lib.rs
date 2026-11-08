//! Subsecond-derived hot-patch protocol (D-10, §11.3): one patch dylib + an
//! atomic jump table; call sites are explicit safepoints; stale frames unwind
//! to the nearest outer safepoint. `never dlclose` — old dylibs stay mapped
//! forever (§2.4's hard-won lesson).
//!
//! M0 spike scope: the protocol skeleton on a dummy host — jump table, patch
//! application, manifest, safepoint call, and the save→frame budget
//! measurement. M3 ships the ThinLink engine on top ([`session`]): chained
//! generations, stub thunks for new symbols, and the never-dlclose proof —
//! the two-generation spike gate (`lig patch-spike`) exercises all of it.

// The crate is the single unsafe module by policy (§15): the dlopen jump
// table and transmutes are its whole surface. `expect` keeps the workspace
// `unsafe_code = "deny"` honest — the expectation warns if unsafe ever dries up.
#![expect(unsafe_code)]
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod manifest;
#[cfg(unix)]
pub mod session;
pub mod table;

pub use manifest::PatchManifest;
#[cfg(unix)]
pub use session::{AppliedPatch, PatchSession, SymbolKind, call_captured, captured_address};
pub use table::{
    FRAME_COUNT, FRAME_LABEL, JumpTable, PatchError, PatchReceipt, apply_patch, commit_table,
    current_table, hot_call,
};

/// The runtime generation of the jump table — bumped on every applied patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildId(pub u64);

impl BuildId {
    /// The initial (fat-build) generation.
    pub const INITIAL: BuildId = BuildId(0);
}

/// Convert a raw address to the manifest's hex form.
#[must_use]
pub fn hex_addr(addr: u64) -> String {
    format!("{addr:#018x}")
}

/// Parse a manifest hex address.
#[must_use]
pub fn parse_hex_addr(s: &str) -> Option<u64> {
    u64::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
}
