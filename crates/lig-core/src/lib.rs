//! Ligature core: ids, spans, interning, error-code registry, stable hashing.
//!
//! Everything the compiler shares before any IR exists (§4.2 `lig-core`):
//! `u32`-backed newtype ids, 8-byte spans matching oxc's layout, an
//! insertion-stable string table with canonical sorting, an FNV-1a hasher for
//! reproducible fingerprints, and the generated diagnostic registry (D-7).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod diagnostic;
pub mod error_code;
pub mod hash;
pub mod id;
pub mod intern;
pub mod span;
