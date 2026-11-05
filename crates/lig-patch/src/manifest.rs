//! `patch.toml` manifest (§11.3 step 3): dylib path + old→new map +
//! `aslr_reference` + build id. Written by the apply step; read by local hosts
//! and the dev channel (remote hosts get it with pid/build-id filtering).

use crate::table::JumpTable;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// A patch manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchManifest {
    /// Path of the patch dylib (as loaded).
    pub dylib: String,
    /// Monotonic build id (generation).
    pub build_id: u64,
    /// The ASLR anchor used to rebase (`main`'s runtime address).
    pub aslr_reference: u64,
    /// old runtime addr (hex) → new file-relative addr (hex).
    pub entries: BTreeMap<String, String>,
}

impl PatchManifest {
    /// Serialize to the minimal TOML subset the M0 parser accepts.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        out.push_str("dylib = \"");
        out.push_str(&self.dylib.replace('\\', "\\\\"));
        out.push_str("\"\n");
        let _ = writeln!(out, "build_id = {}", self.build_id);
        let _ = writeln!(
            out,
            "aslr_reference = \"{}\"",
            crate::hex_addr(self.aslr_reference)
        );
        out.push_str("[entries]\n");
        for (old, new) in &self.entries {
            let _ = writeln!(out, "{old} = \"{new}\"");
        }
        out
    }

    /// Parse the minimal TOML subset (flat keys + one `[entries]` table).
    ///
    /// # Errors
    /// A message describing the first malformed line.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut dylib = None;
        let mut build_id = None;
        let mut aslr_reference = None;
        let mut entries = BTreeMap::new();
        let mut in_entries = false;
        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line == "[entries]" {
                in_entries = true;
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(format!("line {}: expected `key = value`", n + 1));
            };
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            if in_entries {
                entries.insert(key.to_string(), value.to_string());
            } else {
                match key {
                    "dylib" => dylib = Some(value.to_string()),
                    "build_id" => {
                        build_id = Some(
                            value
                                .parse::<u64>()
                                .map_err(|e| format!("line {}: {e}", n + 1))?,
                        );
                    }
                    "aslr_reference" => {
                        aslr_reference = Some(
                            crate::parse_hex_addr(value)
                                .ok_or_else(|| format!("line {}: bad hex", n + 1))?,
                        );
                    }
                    other => return Err(format!("line {}: unknown key {other}", n + 1)),
                }
            }
        }
        Ok(Self {
            dylib: dylib.ok_or("missing dylib")?,
            build_id: build_id.ok_or("missing build_id")?,
            aslr_reference: aslr_reference.ok_or("missing aslr_reference")?,
            entries,
        })
    }

    /// Build the manifest from a receipt + generation counter.
    #[must_use]
    pub fn from_receipt(
        dylib: &std::path::Path,
        build_id: u64,
        receipt: &crate::table::PatchReceipt,
    ) -> Self {
        Self {
            dylib: dylib.display().to_string(),
            build_id,
            aslr_reference: receipt.aslr_reference,
            entries: receipt
                .file_relative
                .iter()
                .map(|(k, v)| (k.clone(), crate::hex_addr(*v)))
                .collect(),
        }
    }
}

/// Convert a committed jump table into manifest entries (runtime→runtime form;
/// the rebased form is what [`PatchManifest::from_receipt`] records).
#[must_use]
pub fn entries_from_table(table: &JumpTable) -> BTreeMap<String, String> {
    table
        .as_map()
        .iter()
        .map(|(old, new)| (crate::hex_addr(*old), crate::hex_addr(*new)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{PatchManifest, entries_from_table};
    use crate::table::JumpTable;
    use std::collections::BTreeMap;

    #[test]
    fn toml_roundtrip() {
        let m = PatchManifest {
            dylib: "target/patch-tip.so".to_string(),
            build_id: 7,
            aslr_reference: 0x0000_5555_0000_1234,
            entries: BTreeMap::from([(
                "0x0000000000010000".to_string(),
                "0x0000000000020000".to_string(),
            )]),
        };
        let toml = m.to_toml();
        assert!(toml.contains("build_id = 7"));
        assert!(toml.contains("aslr_reference = \"0x0000555500001234\""));
        let parsed = PatchManifest::parse(&toml).expect("parses");
        assert_eq!(parsed, m);
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(PatchManifest::parse("nope").is_err());
        assert!(PatchManifest::parse("build_id = x").is_err());
        assert!(
            PatchManifest::parse("dylib = \"a\"").is_err(),
            "missing build_id"
        );
    }

    #[test]
    fn entries_from_table_roundtrips_hex() {
        let t = JumpTable::new(BTreeMap::from([(0x1000, 0x2000)]));
        let entries = entries_from_table(&t);
        assert_eq!(
            entries.get("0x0000000000001000").unwrap(),
            "0x0000000000002000"
        );
    }
}
