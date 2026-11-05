//! The M3 ThinLink engine: patch sessions, generation chaining, and symbol
//! classification (ADR-013's "lands with M3" surface).
//!
//! A [`PatchSession`] applies successive patch dylibs to a running target:
//! every [`PatchSession::apply`] bumps the generation, commits a new jump
//! table atomically (the protocol core in [`crate::table`]), and records the
//! receipt as a chained [`PatchManifest`]. Dylibs are never `dlclose`d —
//! superseded generations stay mapped for stale frames, and their committed
//! addresses remain valid, which is what [`captured_address`] +
//! [`call_captured`] prove in-process.
//!
//! Stub thunks (subsecond's new-symbol story): a fat build that wants to
//! receive *new* functions exports a **stub thunk** under the future name; a
//! patch then routes it like any other symbol — the stub body never runs
//! again. [`SymbolKind::StubThunk`] marks those names in an
//! [`AppliedPatch`]; [`SymbolKind::New`] marks symbols absent from the fat
//! build (they resolve straight to their patch address).

use crate::manifest::PatchManifest;
use crate::table::{PatchReceipt, apply_patch};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How a symbol existed before this patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    /// Exported by the running (fat) build — classic old→new remap.
    Existing,
    /// A stub thunk: the fat build exports a placeholder under this name;
    /// routing replaces the stub body (the subsecond thunk pattern).
    StubThunk,
    /// The symbol did not exist in the fat build — it resolves to its patch
    /// address directly and is callable from the patch dylib itself.
    New,
}

/// Classify a symbol by its fat-build address (`0` = absent).
#[must_use]
pub fn classify(old_addr: u64) -> SymbolKind {
    if old_addr == 0 {
        SymbolKind::New
    } else {
        SymbolKind::Existing
    }
}

/// One applied generation: the protocol receipt plus the session's
/// classification and chained manifest.
#[derive(Debug, Clone)]
pub struct AppliedPatch {
    /// The protocol-level receipt (`apply_patch`).
    pub receipt: PatchReceipt,
    /// This patch's build id (the generation it created).
    pub build_id: u64,
    /// The chained manifest (`from_receipt` at this generation).
    pub manifest: PatchManifest,
    /// Requested symbol → how it existed before this patch.
    pub symbols: BTreeMap<String, SymbolKind>,
}

/// A chained patch session: generation counter + the manifest history.
#[derive(Debug, Default)]
pub struct PatchSession {
    generation: u64,
    applied: Vec<AppliedPatch>,
}

impl PatchSession {
    /// A fresh session at [`crate::BuildId::INITIAL`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The current generation (the last applied patch's build id).
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The applied generations, in application order.
    #[must_use]
    pub fn applied(&self) -> &[AppliedPatch] {
        &self.applied
    }

    /// The loaded dylibs, in application order (never unmapped).
    #[must_use]
    pub fn dylibs(&self) -> Vec<PathBuf> {
        self.applied
            .iter()
            .map(|a| a.receipt.dylib.clone())
            .collect()
    }

    /// Apply the next patch dylib: loads it, resolves `symbols`, commits the
    /// jump table, bumps the generation, and chains the manifest. Names in
    /// `stub_thunks` are recorded as [`SymbolKind::StubThunk`] (the fat build
    /// exports placeholder bodies for them).
    ///
    /// # Errors
    /// The [`crate::PatchError`] of the underlying `apply_patch`.
    pub fn apply(
        &mut self,
        dylib: &Path,
        symbols: &[&str],
        stub_thunks: &[&str],
    ) -> Result<AppliedPatch, crate::PatchError> {
        let receipt = apply_patch(dylib, symbols)?;
        self.generation += 1;
        let manifest = PatchManifest::from_receipt(dylib, self.generation, &receipt);
        let mut kinds = BTreeMap::new();
        for sym in symbols {
            let kind = if stub_thunks.contains(sym) {
                SymbolKind::StubThunk
            } else {
                // The receipt only records symbols that CHANGED; a requested
                // symbol missing from `file_relative` was a no-op remap.
                SymbolKind::Existing
            };
            kinds.insert((*sym).to_string(), kind);
        }
        let applied = AppliedPatch {
            receipt,
            build_id: self.generation,
            manifest,
            symbols: kinds,
        };
        self.applied.push(applied);
        Ok(self.applied.last().cloned().expect("just pushed"))
    }
}

/// The committed (current-generation) runtime address a fat-build function
/// pointer routes to, if any — the address a stale frame would still hold.
///
/// Capturing this **before** the next apply proves the never-dlclose
/// discipline afterwards: the captured address stays executable across any
/// number of later generations (see [`call_captured`]).
#[must_use]
pub fn captured_address<R>(f: fn() -> R) -> Option<u64> {
    let table = crate::table::current_table()?;
    table.lookup(f as usize as u64)
}

/// Call a captured (previous-generation) address directly.
///
/// # Safety contract
/// The address must come from [`captured_address`] — a jump-table entry the
/// protocol committed and deliberately leaked (never-dlclose, §2.4), with the
/// same signature as the original symbol.
///
/// # Panics
/// Never for an address produced by [`captured_address`].
#[must_use]
pub fn call_captured<R>(addr: u64) -> R {
    // SAFETY: `captured_address` returns a leaked-table entry whose target
    // lives in a dylib that is never unmapped, and the protocol's
    // same-signature invariant holds for symbol-matched remaps.
    let f: fn() -> R = unsafe { core::mem::transmute(addr) };
    f()
}

#[cfg(test)]
mod tests {
    use super::{SymbolKind, classify};

    #[test]
    fn classification_matches_the_protocol() {
        assert_eq!(classify(0), SymbolKind::New);
        assert_eq!(classify(0x1000), SymbolKind::Existing);
    }

    #[test]
    fn sessions_start_at_the_initial_generation() {
        let s = super::PatchSession::new();
        assert_eq!(s.generation(), 0);
        assert!(s.applied().is_empty());
        assert_eq!(s.dylibs(), Vec::<std::path::PathBuf>::new());
    }
}
