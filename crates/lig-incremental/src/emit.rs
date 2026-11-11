//! Digest-gated emission (D-9): `dist/` artifacts are written only when a
//! unit's content digest changed (or the artifact is missing); unchanged
//! units are **kept** byte-for-byte. The pass records per-unit digests in
//! `build-state.json`, making the next pass — `lig build` or a `lig watch`
//! tick — able to skip the write entirely.
//!
//! Fail-closed ordering (BR-1) is the caller's job: nothing here runs until
//! every unit in the pass is admitted.

use crate::CompiledUnit;
use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where emitted artifacts land (the emission-target seam): [`emit_units_to`]
/// writes the §9.5 artifact set through this object-safe trait, so digest-gated
/// emission (D-9) is not bound to the filesystem. [`FsSink`] is the production
/// target (`lig build` and `lig watch` write `dist/` through it, atomically);
/// [`MemSink`] keeps artifacts in a caller-owned map for tests and embedded
/// hosts with no filesystem. Implement the trait to hand the artifacts to any
/// other store — an asset pipeline, a virtual FS, a network cache.
///
/// Contract: paths are slash-separated artifact paths relative to the emission
/// root (`<stem>.rs`, `ir/<stem>.json`, `maps/<stem>.map.json`,
/// `manifest.toml`, `build-state.json`); `write` must be atomic or otherwise
/// never leave a half-written artifact at `path` — the digest gate trusts the
/// recorded state to match what `read` returns.
pub trait ArtifactSink {
    /// The sink's label: the `emit` span's `sink` field, so a trace says
    /// where the artifacts went.
    fn label(&self) -> Cow<'_, str>;

    /// The error subject for one artifact path (what [`EmitError`] names as
    /// the failing unit or artifact). The default joins the label and the
    /// path; a sink may use its own scheme.
    fn subject(&self, path: &str) -> String {
        format!("{}/{}", self.label(), path)
    }

    /// The bytes at `path`, if the sink holds them (the previous
    /// `build-state.json` is loaded through this).
    fn read(&self, path: &str) -> Option<Vec<u8>>;

    /// Write (or replace) the artifact at `path` — atomically.
    ///
    /// # Errors
    /// Propagate the store's failure; [`emit_units_to`] maps it onto the
    /// typed [`EmitError`] (`ligature::emit`, with help-text).
    fn write(&mut self, path: &str, bytes: &[u8]) -> std::io::Result<()>;

    /// Drop the artifact at `path` if present — best-effort by contract:
    /// the watch prune never fails on a missing artifact.
    fn remove(&mut self, path: &str);

    /// Whether the sink currently holds `path` — the state/artifact
    /// mismatch check (a recorded digest with a missing artifact re-emits:
    /// the state is a cache, never a lie).
    fn exists(&self, path: &str) -> bool;
}

/// The filesystem sink: artifacts land under `root` in the exact `dist/`
/// layout `lig build` has always written — `write` is the atomic
/// temporary-then-rename two-step, and `read`/`exists` back the digest
/// gate. This is what [`emit_units`] drives.
#[derive(Debug, Clone)]
pub struct FsSink {
    root: PathBuf,
}

impl FsSink {
    /// A sink rooted at `root` (directories are created on demand by
    /// `write`, one level per artifact path).
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn full(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }
}

impl ArtifactSink for FsSink {
    fn label(&self) -> Cow<'_, str> {
        Cow::Owned(self.root.display().to_string())
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(self.full(path)).ok()
    }

    fn write(&mut self, path: &str, bytes: &[u8]) -> std::io::Result<()> {
        let full = self.full(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // The atomic two-step: write the temporary, then rename over the
        // artifact (the `.tmp` never outlives this call on success).
        let tmp = full.with_extension("tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &full)
    }

    fn remove(&mut self, path: &str) {
        drop(std::fs::remove_file(self.full(path)));
    }

    fn exists(&self, path: &str) -> bool {
        self.full(path).is_file()
    }
}

/// The in-memory sink: artifacts land in a caller-owned `BTreeMap`
/// (artifact path → bytes). Writes are atomic by construction (map insert)
/// and the digest gate reads back through the same map — emission without a
/// filesystem, for tests, playgrounds, and embedded hosts that hand the
/// artifacts to their own loader.
#[derive(Debug, Default, Clone)]
pub struct MemSink {
    artifacts: BTreeMap<String, Vec<u8>>,
}

impl MemSink {
    /// An empty sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The bytes at one artifact path, if emitted.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&[u8]> {
        self.artifacts.get(path).map(Vec::as_slice)
    }

    /// The emitted artifact paths, sorted.
    #[must_use]
    pub fn paths(&self) -> Vec<&str> {
        self.artifacts.keys().map(String::as_str).collect()
    }

    /// Take the artifacts (artifact path → bytes).
    #[must_use]
    pub fn into_artifacts(self) -> BTreeMap<String, Vec<u8>> {
        self.artifacts
    }
}

impl ArtifactSink for MemSink {
    fn label(&self) -> Cow<'_, str> {
        Cow::Borrowed("mem")
    }

    fn subject(&self, path: &str) -> String {
        format!("mem://{path}")
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.artifacts.get(path).cloned()
    }

    fn write(&mut self, path: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.artifacts.insert(path.to_string(), bytes.to_vec());
        Ok(())
    }

    fn remove(&mut self, path: &str) {
        self.artifacts.remove(path);
    }

    fn exists(&self, path: &str) -> bool {
        self.artifacts.contains_key(path)
    }
}

/// The persisted per-unit digests (`dist/build-state.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildState {
    /// State schema version.
    pub version: u32,
    /// The admission profile the pass ran with (`taffy` | `web`).
    pub profile: String,
    /// Per stem: source path + content digests.
    pub units: BTreeMap<String, UnitState>,
}

/// One unit's recorded digests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitState {
    /// Source path as registered.
    pub source: String,
    /// FNV-1a 64 of the source text.
    pub source_hash: String,
    /// FNV-1a 64 of the generated Rust text.
    pub text_hash: String,
    /// FNV-1a 64 of the emitted IR JSON.
    pub ir_hash: String,
    /// FNV-1a 64 of the emitted merged source map JSON.
    pub map_hash: String,
}

/// What one emission pass did.
#[derive(Debug, Default, Clone)]
pub struct EmissionReport {
    /// Stems whose artifacts were (re)written.
    pub written: Vec<String>,
    /// Stems whose artifacts were kept (digest unchanged — D-9).
    pub kept: Vec<String>,
    /// Stems pruned (source removed from the session; watch sync).
    pub pruned: Vec<String>,
    /// NFR-6 verification stats per written stem (for the report lines).
    pub stats: BTreeMap<String, lig_sourcemap::VerifyStats>,
}

/// An emission failure (the `dist/` writer's typed seam, M16): which unit or
/// artifact failed, why, and the fix in words — a [`miette::Diagnostic`] with
/// a stable code (`ligature::emit`) and help-text, so embedded consumers get
/// the same quality of failure report the CLI prints.
#[derive(Debug)]
pub struct EmitError {
    /// The unit stem or artifact path the failure is about.
    pub subject: String,
    /// What went wrong (renders as the error line).
    pub message: String,
    /// The fix, in words (renders as `help:`).
    pub help: String,
    /// The underlying I/O error, when one caused this.
    io: Option<std::io::Error>,
}

impl EmitError {
    /// A sink failure against one artifact path (the atomic write, the
    /// directory creation, the state file) — the subject comes from the
    /// sink (for the filesystem sink, the full artifact path).
    fn io(subject: String, e: std::io::Error) -> Self {
        Self {
            message: format!("{subject}: {e}"),
            subject,
            help: "emission writes artifacts atomically (write a temporary, then rename) — \
                   check that the sink's backing store is writable and that no read-only \
                   mount, permission change, or stale `.tmp` file blocks the rename"
                .to_string(),
            io: Some(e),
        }
    }

    /// A contract violation on one unit (fail-closed BR-1 or a failed map
    /// verification — the pipeline's own invariants, not the filesystem).
    fn contract(stem: &str, message: String, help: String) -> Self {
        Self {
            subject: stem.to_string(),
            message,
            help,
            io: None,
        }
    }
}

impl core::fmt::Display for EmitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.io
            .as_ref()
            .map(|e| e as &(dyn std::error::Error + 'static))
    }
}

impl Diagnostic for EmitError {
    fn code<'a>(&'a self) -> Option<Box<dyn core::fmt::Display + 'a>> {
        Some(Box::new("ligature::emit"))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn core::fmt::Display + 'a>> {
        Some(Box::new(&self.help))
    }
}

/// Load the previous build state, if any.
#[must_use]
pub fn load_state(out_dir: &Path) -> Option<BuildState> {
    let text = std::fs::read_to_string(out_dir.join("build-state.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Emit `units` into the `out_dir` filesystem sink with digest gating:
///
/// - `dist/<stem>.rs`, `dist/ir/<stem>.json`, `dist/maps/<stem>.map.json`
///   written only when their content hash differs from the recorded state
///   (or the file is missing — a state/artifact mismatch re-emits: the state
///   is a cache, never a lie);
/// - `dist/manifest.toml` rewritten every pass (it lists the current units);
/// - `dist/build-state.json` records this pass's digests;
/// - with `prune`, artifacts of stems not in `units` are removed (the watch
///   loop keeps `dist` synced with the source set).
///
/// This is the filesystem specialization of [`emit_units_to`] — the same
/// shared body drives any [`ArtifactSink`].
///
/// # Errors
/// A typed [`EmitError`] on any I/O failure or a failed NFR-6 source-map
/// verification (fail-closed, naming the offending stem) — `code`
/// `ligature::emit`, with miette help-text naming the fix.
pub fn emit_units(
    out_dir: &Path,
    units: &[&CompiledUnit],
    profile: &str,
    prune: bool,
) -> Result<EmissionReport, EmitError> {
    let mut sink = FsSink::new(out_dir.to_path_buf());
    emit_units_to(&mut sink, units, profile, prune)
}

/// Emit `units` into any [`ArtifactSink`] with the digest gating of
/// [`emit_units`] — the emission-target seam. The artifact set, the digests,
/// and the report are sink-independent: the same unit set emits byte-for-byte
/// identical artifacts through the filesystem sink and the memory sink.
///
/// # Errors
/// A typed [`EmitError`] on any sink write failure or a failed NFR-6
/// source-map verification (fail-closed, naming the offending stem) —
/// `code` `ligature::emit`, with miette help-text naming the fix.
pub fn emit_units_to(
    sink: &mut dyn ArtifactSink,
    units: &[&CompiledUnit],
    profile: &str,
    prune: bool,
) -> Result<EmissionReport, EmitError> {
    let span = tracing::info_span!(
        "emit",
        units = units.len(),
        sink = %sink.label(),
        written = tracing::field::Empty,
        kept = tracing::field::Empty,
    );
    let _guard = span.enter();
    let prev = sink
        .read("build-state.json")
        .and_then(|bytes| serde_json::from_slice::<BuildState>(&bytes).ok());
    let mut report = EmissionReport::default();

    let mut state = BuildState {
        version: 1,
        profile: profile.to_string(),
        units: BTreeMap::new(),
    };
    let mut manifest_rows: Vec<String> = vec![
        "# generated by lig build — do not edit".to_string(),
        format!("profile = \"{profile}\""),
        format!("units = {}", units.len()),
    ];

    for unit in units {
        let Some(out) = unit.generated.as_ref() else {
            continue; // denied units never reach emission (BR-1 upstream)
        };
        let module = unit.module.as_ref().ok_or_else(|| {
            EmitError::contract(
                &unit.stem,
                format!("{}: admitted unit without IR", unit.stem),
                "an admitted unit always carries its lowered IR — the fail-closed contract \
                 (BR-1) makes this an internal bug: re-run the pass and report it with the \
                 session revision from `lig doctor`"
                    .to_string(),
            )
        })?;

        // NFR-6: verify the chained maps before any emission.
        let verified =
            lig_sourcemap::verify(module, &unit.source, &out.text, &out.links).map_err(|e| {
                EmitError::contract(
                    &unit.stem,
                    format!("{}: sourcemap verify failed: {e}", unit.stem),
                    "the chained map failed its own verification — an internal mapping bug: \
                     run `lig reproducibility` to pin it against the committed unit"
                        .to_string(),
                )
            })?;

        let src_map = lig_sourcemap::src_to_ir(module, &unit.source);
        let ir_map = lig_sourcemap::ir_to_rust(module, &out.text, &out.links).map_err(|e| {
            EmitError::contract(
                &unit.stem,
                format!("{}: IR→Rust map failed: {e}", unit.stem),
                "the generated-symbol links did not resolve — an internal mapping bug: run \
                 `lig resolve` on the committed map to narrow it"
                    .to_string(),
            )
        })?;
        let merged = lig_sourcemap::chain(&src_map, &ir_map);

        let rs_bytes = out.text.as_bytes();
        let ir_json = format!("{}\n", module.canonical_json_pretty());
        let map_json = merged.to_json_string();
        let ir_bytes = ir_json.as_bytes();
        let map_bytes = map_json.as_bytes();

        let ir_hash = crate::digest(ir_bytes);
        let map_hash = crate::digest(map_bytes);
        let entry = UnitState {
            source: unit.path.display().to_string(),
            source_hash: hex(unit.source_hash()),
            text_hash: hex(out.digest),
            ir_hash: hex(ir_hash),
            map_hash: hex(map_hash),
        };

        let unchanged = prev.as_ref().is_some_and(|prev| {
            prev.units.get(&unit.stem).is_some_and(|old| {
                *old == entry
                    && sink.exists(&format!("{}.rs", unit.stem))
                    && sink.exists(&format!("ir/{}.json", unit.stem))
                    && sink.exists(&format!("maps/{}.map.json", unit.stem))
            })
        });
        if unchanged {
            report.kept.push(unit.stem.clone());
        } else {
            let rs_path = format!("{}.rs", unit.stem);
            sink.write(&rs_path, rs_bytes)
                .map_err(|e| EmitError::io(sink.subject(&rs_path), e))?;
            let ir_path = format!("ir/{}.json", unit.stem);
            sink.write(&ir_path, ir_bytes)
                .map_err(|e| EmitError::io(sink.subject(&ir_path), e))?;
            let map_path = format!("maps/{}.map.json", unit.stem);
            sink.write(&map_path, map_bytes)
                .map_err(|e| EmitError::io(sink.subject(&map_path), e))?;
            report.written.push(unit.stem.clone());
            report.stats.insert(unit.stem.clone(), verified);
        }
        state.units.insert(unit.stem.clone(), entry);
        manifest_rows.push(format!("{} = \"{}.rs\"", unit.stem, unit.stem));
    }

    // Watch sync: drop artifacts for sources that left the session. Removal
    // is best-effort (the sink contract): a missing artifact is not an error.
    if let Some(prev) = prev.as_ref().filter(|_| prune) {
        for stem in prev.units.keys() {
            if !state.units.contains_key(stem) {
                sink.remove(&format!("{stem}.rs"));
                sink.remove(&format!("ir/{stem}.json"));
                sink.remove(&format!("maps/{stem}.map.json"));
                report.pruned.push(stem.clone());
            }
        }
    }

    span.record("written", report.written.len());
    span.record("kept", report.kept.len());
    sink.write("manifest.toml", manifest_rows.join("\n").as_bytes())
        .map_err(|e| EmitError::io(sink.subject("manifest.toml"), e))?;
    let state_json = serde_json::to_string_pretty(&state).map_err(|e| {
        EmitError::contract(
            "build-state.json",
            format!("serialize build state: {e}"),
            "the build state is plain serde JSON over owned digests — a serialization failure \
             here is an internal bug: report it with the session revision"
                .to_string(),
        )
    })?;
    sink.write("build-state.json", state_json.as_bytes())
        .map_err(|e| EmitError::io(sink.subject("build-state.json"), e))?;
    Ok(report)
}

fn hex(v: u64) -> String {
    format!("{v:#018x}")
}

#[cfg(test)]
mod tests {
    use super::{ArtifactSink, MemSink, emit_units, emit_units_to, load_state};
    use crate::Session;
    use lig_admission::Profile;
    use miette::Diagnostic;
    use std::path::PathBuf;

    fn counter_session() -> Session {
        let mut s = Session::new();
        s.set_source(
            PathBuf::from("counter.tsx"),
            lig_parse_solid::tests_support::COUNTER_TSX,
        );
        s.compile(Profile::Layout);
        s
    }

    /// The five artifact paths one counter emission lands, in the §9.5 layout.
    fn artifact_paths() -> [String; 5] {
        [
            "counter.rs".to_string(),
            "ir/counter.json".to_string(),
            "maps/counter.map.json".to_string(),
            "manifest.toml".to_string(),
            "build-state.json".to_string(),
        ]
    }

    #[test]
    fn second_pass_keeps_written_artifacts() {
        let s = counter_session();
        let out = std::env::temp_dir().join(format!("lig-emit-test-{}-keep", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let units = s.units();
        let first = emit_units(&out, &units, "taffy", false).expect("first emission writes");
        assert_eq!(first.written, vec!["counter".to_string()]);
        assert!(first.kept.is_empty(), "the first pass writes, never keeps");
        let second = emit_units(&out, &units, "taffy", false).expect("second emission");
        assert!(
            second.written.is_empty(),
            "digest unchanged — the artifacts are kept (D-9)"
        );
        assert_eq!(second.kept, vec!["counter".to_string()]);
        assert!(
            load_state(&out).is_some(),
            "the state file records the pass"
        );
        std::fs::remove_dir_all(&out).ok();
    }

    #[test]
    fn emission_failure_is_a_typed_error_with_help() {
        let s = counter_session();
        // A regular file where the out directory should be: create_dir_all
        // fails, and the failure surfaces as a typed `EmitError` (code +
        // miette help-text), not a bare `String`.
        let blocker =
            std::env::temp_dir().join(format!("lig-emit-test-{}-blocker", std::process::id()));
        std::fs::write(&blocker, b"not a directory").expect("write blocker");
        let out = blocker.join("dist");
        let units = s.units();
        let err = emit_units(&out, &units, "taffy", false).expect_err("blocked emission fails");
        assert!(
            err.message.contains("dist"),
            "the message names the blocked path: {err}"
        );
        assert!(
            err.code().map(|c| c.to_string()).as_deref() == Some("ligature::emit"),
            "a stable code: {err:?}"
        );
        let help = err
            .help()
            .map(|h| h.to_string())
            .expect("miette help-text present");
        assert!(help.contains("writable"), "the help names the fix: {help}");
        std::fs::remove_file(&blocker).ok();
    }

    #[test]
    fn mem_sink_emits_the_full_artifact_set_and_keeps_it() {
        // The emission-target seam: `emit_units_to` drives ANY sink — here
        // the in-memory one, no filesystem involved. The artifact set is the
        // §9.5 layout, the digest gate keeps the second pass byte-for-byte.
        let s = counter_session();
        let units = s.units();
        let mut sink = MemSink::new();
        let first = emit_units_to(&mut sink, &units, "taffy", false).expect("mem emission writes");
        assert_eq!(first.written, vec!["counter".to_string()]);
        for path in artifact_paths() {
            let bytes = sink
                .get(&path)
                .unwrap_or_else(|| panic!("{path} emitted into the sink"));
            assert!(!bytes.is_empty(), "{path} is non-empty");
        }
        // The generated unit in the sink is THE counter unit: byte-identical
        // to what the filesystem sink writes (checked against the session).
        let generated = s
            .unit(std::path::Path::new("counter.tsx"))
            .and_then(|u| u.generated.as_ref())
            .expect("an admitted unit carries its output");
        assert_eq!(sink.get("counter.rs"), Some(generated.text.as_bytes()));
        // Second pass: the recorded digests still validate, nothing rewrites.
        let second = emit_units_to(&mut sink, &units, "taffy", false).expect("mem re-emission");
        assert!(second.written.is_empty(), "digest-gated: nothing re-wrote");
        assert_eq!(second.kept, vec!["counter".to_string()]);
    }

    #[test]
    fn mem_sink_reemits_after_an_edit() {
        // A source edit invalidates the recorded digests: the next in-memory
        // emission rewrites the unit — the D-9 gate works without a disk.
        let mut s = counter_session();
        let mut sink = MemSink::new();
        let units = s.units();
        emit_units_to(&mut sink, &units, "taffy", false).expect("first emission");
        let edited = lig_parse_solid::tests_support::COUNTER_TSX.replace("Add", "Add2");
        assert_ne!(edited, lig_parse_solid::tests_support::COUNTER_TSX);
        s.set_source(PathBuf::from("counter.tsx"), edited);
        let _ = s.compile(Profile::Layout);
        let units = s.units();
        let pass = emit_units_to(&mut sink, &units, "taffy", false).expect("re-emission");
        assert_eq!(
            pass.written,
            vec!["counter".to_string()],
            "the edit re-emits"
        );
        assert_eq!(pass.kept, Vec::<String>::new(), "only the edit re-emits");
    }

    #[test]
    fn mem_sink_prunes_removed_units() {
        // Watch sync: a unit that left the session has its artifacts pruned
        // from the sink (with `prune`, the same contract the fs sink holds).
        let mut s = Session::new();
        s.set_source(
            PathBuf::from("a.tsx"),
            lig_parse_solid::tests_support::COUNTER_TSX,
        );
        s.set_source(
            PathBuf::from("b.tsx"),
            lig_parse_solid::tests_support::COUNTER_TSX,
        );
        s.compile(Profile::Layout);
        let mut sink = MemSink::new();
        let units = s.units();
        let first = emit_units_to(&mut sink, &units, "taffy", false).expect("two units emitted");
        assert_eq!(
            first.written.len(),
            2,
            "both stems written: {:?}",
            first.written
        );
        s.remove_source(std::path::Path::new("b.tsx"));
        let _ = s.compile(Profile::Layout);
        let units = s.units();
        let second = emit_units_to(&mut sink, &units, "taffy", true).expect("prune pass");
        assert!(second.written.is_empty(), "a.tsx digest still holds");
        assert_eq!(second.pruned, vec!["b".to_string()]);
        assert!(
            sink.get("b.rs").is_none()
                && sink.get("ir/b.json").is_none()
                && sink.get("maps/b.map.json").is_none(),
            "b's artifact set is gone from the sink"
        );
        assert!(sink.get("a.rs").is_some(), "a's artifacts survive");
    }

    #[test]
    fn fs_and_mem_sinks_agree_byte_for_byte() {
        // The two sinks run the SAME shared emission body: the artifacts must
        // be identical bytes — the seam moves where artifacts land, never
        // what they contain.
        let s = counter_session();
        let units = s.units();
        let mut mem = MemSink::new();
        emit_units_to(&mut mem, &units, "taffy", false).expect("mem emission");
        let fs_dir =
            std::env::temp_dir().join(format!("lig-emit-test-{}-parity", std::process::id()));
        let _ = std::fs::remove_dir_all(&fs_dir);
        emit_units(&fs_dir, &units, "taffy", false).expect("fs emission");
        for path in artifact_paths() {
            let fs_bytes =
                std::fs::read(fs_dir.join(&path)).unwrap_or_else(|e| panic!("read {path}: {e}"));
            assert_eq!(
                mem.get(&path),
                Some(fs_bytes.as_slice()),
                "{path} is byte-identical across sinks"
            );
        }
        std::fs::remove_dir_all(&fs_dir).ok();
    }

    /// A sink whose writes always fail: proves the seam maps sink failures
    /// onto the typed `EmitError` (`ligature::emit`, help-text) — a caller's
    /// sink gets the same quality of failure report the filesystem gets.
    struct FailingSink;

    impl ArtifactSink for FailingSink {
        fn label(&self) -> std::borrow::Cow<'_, str> {
            std::borrow::Cow::Borrowed("failing")
        }

        fn subject(&self, path: &str) -> String {
            format!("failing://{path}")
        }

        fn read(&self, _path: &str) -> Option<Vec<u8>> {
            None
        }

        fn write(&mut self, _path: &str, _bytes: &[u8]) -> std::io::Result<()> {
            Err(std::io::Error::other("the sink refuses"))
        }

        fn remove(&mut self, _path: &str) {}

        fn exists(&self, _path: &str) -> bool {
            false
        }
    }

    #[test]
    fn a_failing_sink_surfaces_the_typed_emit_error() {
        let s = counter_session();
        let units = s.units();
        let mut sink = FailingSink;
        let err = emit_units_to(&mut sink, &units, "taffy", false)
            .expect_err("a refusing sink fails the emission");
        assert!(
            err.message.contains("counter.rs"),
            "names the artifact: {err}"
        );
        assert!(
            err.code().map(|c| c.to_string()).as_deref() == Some("ligature::emit"),
            "a stable code: {err:?}"
        );
        let help = err
            .help()
            .map(|h| h.to_string())
            .expect("miette help-text present");
        assert!(help.contains("writable"), "the help names the fix: {help}");
    }
}
