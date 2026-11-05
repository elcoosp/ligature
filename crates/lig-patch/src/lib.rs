//! Subsecond-derived hot-patch protocol (D-10, §11.3): one patch dylib + an
//! atomic jump table; call sites are explicit safepoints; stale frames unwind
//! to the nearest outer safepoint. `never dlclose` — old dylibs stay mapped
//! forever (§2.4's hard-won lesson).
//!
//! M0 spike scope: the protocol skeleton on a dummy host — jump table, patch
//! application, manifest, safepoint call, and the save→frame budget
//! measurement. The dx ThinLink machinery (rustc replay + stub thunks) lands
//! with M3's patch engine; this spike proves the protocol shape and the <1 s
//! budget on Linux/macOS.

#![allow(unsafe_code)]
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod manifest;
pub mod table;

pub use manifest::PatchManifest;
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
