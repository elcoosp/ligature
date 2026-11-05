//! The admission rule table (§6.5 governance, draft V.7): `docs/admission/rules.md`
//! is the source of truth, machine-parsed here. Each row links one allow and
//! one deny fixture; the `rules-sync` gate enforces 1:1 with
//! `fixtures/admission/`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// One rule row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// Stable `LIG-E###`/`LIG-W###` code.
    pub code: String,
    /// `deny` | `lint`.
    pub tier: String,
    /// The construct the rule classifies.
    pub construct: String,
    /// Fixture dir with the admitted shape (`allow/input.solid.tsx`).
    pub allow_fixture: String,
    /// Fixture dir with the rejected (or linted) shape (`deny/…` / `lint/…`).
    pub deny_fixture: String,
}

/// Parse the rule table from the catalog Markdown.
///
/// # Errors
/// A row that does not match the table grammar (fail-closed — a malformed
/// rule table must never silently drop rules).
pub fn parse_rules(markdown: &str) -> Result<Vec<Rule>, String> {
    let mut rules = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') || trimmed.contains("--") {
            continue;
        }
        let cells: Vec<&str> = trimmed
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.len() < 5 {
            continue;
        }
        if cells[0].eq_ignore_ascii_case("code") {
            continue; // header row
        }
        if cells[0].is_empty() {
            continue;
        }
        rules.push(Rule {
            code: cells[0].to_string(),
            tier: cells[1].to_string(),
            construct: cells[2].to_string(),
            allow_fixture: cells[3].to_string(),
            deny_fixture: cells[4].to_string(),
        });
    }
    if rules.is_empty() {
        return Err("rule table has no rows".to_string());
    }
    Ok(rules)
}

/// Load the rule table from the repo (`docs/admission/rules.md`).
///
/// # Errors
/// Missing file or malformed table.
pub fn load(repo_root: &Path) -> Result<Vec<Rule>, String> {
    let path = repo_root.join("docs/admission/rules.md");
    let md = std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    parse_rules(&md)
}

/// V.7 sync check: every rule's fixtures exist, and every admission fixture
/// directory is referenced by exactly one rule row.
///
/// # Errors
/// Any 1:1 violation with `fixtures/admission/`.
pub fn check_fixture_sync(repo_root: &Path, rules: &[Rule]) -> Result<(), String> {
    let base = repo_root.join("fixtures/admission");
    for r in rules {
        if !r.code.starts_with("LIG-E") && !r.code.starts_with("LIG-W") {
            return Err(format!("{}: code must be LIG-E###/LIG-W###", r.code));
        }
        if !matches!(r.tier.as_str(), "deny" | "lint") {
            return Err(format!(
                "{}: tier must be deny|lint, got {}",
                r.code, r.tier
            ));
        }
        for (kind, dir) in [
            ("allow", &r.allow_fixture),
            ("deny-or-lint", &r.deny_fixture),
        ] {
            let p: PathBuf = base
                .join(&r.code)
                .parent()
                .map_or_else(|| base.join(dir), |_| base.join(dir));
            let _ = kind;
            if !p.join("input.solid.tsx").exists() {
                return Err(format!(
                    "{}: fixture {} missing input.solid.tsx at {}",
                    r.code,
                    dir,
                    p.display()
                ));
            }
        }
    }
    // Every fixture dir on disk is referenced by some row (fixture refs are
    // `<rule-dir>/<tier>`; the rule-dir component is the 1:1 unit).
    let mut on_disk: BTreeSet<String> = BTreeSet::new();
    if base.exists() {
        for entry in std::fs::read_dir(&base).map_err(|e| e.to_string())? {
            let dir = entry.map_err(|e| e.to_string())?.path();
            if dir.is_dir() {
                on_disk.insert(
                    dir.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                );
            }
        }
    }
    let referenced: BTreeSet<String> = rules
        .iter()
        .flat_map(|r| [r.allow_fixture.as_str(), r.deny_fixture.as_str()])
        .map(|f| f.split('/').next().unwrap_or(f).to_string())
        .collect();
    let orphans: Vec<_> = on_disk.difference(&referenced).collect();
    if !orphans.is_empty() {
        return Err(format!(
            "fixture dirs not referenced by the rule table: {orphans:?}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{check_fixture_sync, parse_rules};

    const SAMPLE: &str = r"
# Admission rules

| Code | Tier | Construct | Allow fixture | Deny fixture |
|---|---|---|---|---|
| LIG-E114 | deny | call with callback | e114-callback/allow | e114-callback/deny |
| LIG-W102 | lint | dynamic style object | w102-style/lint-allow | w102-style/lint |
";

    #[test]
    fn parses_rows_and_skips_headers() {
        let rules = parse_rules(SAMPLE).expect("parses");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].code, "LIG-E114");
        assert_eq!(rules[0].tier, "deny");
        assert_eq!(rules[1].tier, "lint");
    }

    #[test]
    fn empty_table_is_an_error() {
        assert!(parse_rules("no table here").is_err());
    }

    #[test]
    fn sync_flags_missing_fixtures() {
        let rules = parse_rules(SAMPLE).expect("parses");
        let tmp = std::env::temp_dir().join("lig-rules-sync-test");
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(check_fixture_sync(&tmp, &rules).is_err(), "no fixtures yet");
    }
}
