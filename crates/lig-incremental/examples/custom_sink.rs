//! A consumer-implemented `ArtifactSink`: the emission-target seam's
//! contract in your hands. The built-in sinks are the atomic `FsSink`
//! (`dist/`) and the `MemSink` (tests, embedded hosts); the trait doc
//! promises "an asset pipeline, a virtual FS, a network cache" — this
//! walkthrough implements the cache: a content-addressed store that
//! dedupes writes by digest, the way an asset pipeline stages artifacts.
//!
//! The session emission seam rides along: `Session::emit_to` is the
//! one-call front — the BR-1 admission gate and the profile ledger name
//! are enforced inside the call, not at the call site.
//!
//! 1. Compile the M0 reference workload in a [`Session`].
//! 2. Emit through a `DigestCache` — writes land once, keyed by digest.
//! 3. Emit again — the digest gate keeps everything (zero writes).
//! 4. Edit and emit — exactly the edited unit's artifacts rewrite.
//! 5. Show the BR-1 refusal: a denied unit writes NOTHING.
//!
//! Run: `cargo run -p lig-incremental --example custom_sink`

use lig_admission::Profile;
use lig_incremental::Session;
use lig_incremental::digest;
use lig_incremental::emit::{ArtifactSink, EmissionReport};
use miette::Diagnostic as _;
use std::borrow::Cow;
use std::collections::BTreeMap;

/// A content-addressed artifact cache: bytes are stored once under their
/// FNV-1a digest, and `path` entries point at digests. `write` dedupes
/// (an unchanged artifact re-stored costs nothing — the asset-pipeline
/// trick), and `read`/`exists` serve the digest gate from the store.
#[derive(Default)]
struct DigestCache {
    blobs: BTreeMap<u64, Vec<u8>>,
    entries: BTreeMap<String, u64>,
    writes: usize,
    deduped: usize,
}

impl DigestCache {
    fn new() -> Self {
        Self::default()
    }

    /// The bytes at `path`, if the cache holds the entry.
    fn get(&self, path: &str) -> Option<&[u8]> {
        let d = *self.entries.get(path)?;
        self.blobs.get(&d).map(Vec::as_slice)
    }

    /// The distinct blobs staged (the dedupe count a pipeline would report).
    fn blob_count(&self) -> usize {
        self.blobs.len()
    }
}

impl ArtifactSink for DigestCache {
    fn label(&self) -> Cow<'_, str> {
        Cow::Borrowed("digest-cache")
    }

    fn subject(&self, path: &str) -> String {
        format!("cache://{path}")
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.get(path).map(<[u8]>::to_vec)
    }

    fn write(&mut self, path: &str, bytes: &[u8]) -> std::io::Result<()> {
        // Atomic by construction: the entry flips to the new digest only
        // after the blob is staged (the digest gate trusts `read` to match
        // what was written — a map insert never half-lands).
        let d = digest(bytes);
        if self.blobs.insert(d, bytes.to_vec()).is_some() {
            self.deduped += 1;
        }
        self.entries.insert(path.to_string(), d);
        self.writes += 1;
        Ok(())
    }

    fn remove(&mut self, path: &str) {
        // Best-effort by contract: the entry goes, the blob stays (another
        // path — or a later re-emit — may still want the bytes).
        self.entries.remove(path);
    }

    fn exists(&self, path: &str) -> bool {
        self.entries.contains_key(path)
    }
}

fn main() {
    // 1. One cold pass over the counter.
    let source = include_str!("../../../fixtures/conformance/counter/input.solid.tsx");
    let mut session = Session::new();
    session.set_source("counter.tsx", source);
    session.compile(Profile::Layout);

    // 2. The one-call seam into the custom sink: BR-1 and the profile
    //    ledger name ride the call — `emit_to` refuses a denied pass
    //    before the sink sees a single byte.
    let mut cache = DigestCache::new();
    let first: EmissionReport = session
        .emit_to(&mut cache, Profile::Layout, false)
        .expect("the admitted pass emits");
    println!(
        "emit 1: wrote {} artifact set(s) — {} sink writes, {} distinct blobs staged",
        first.written.len(),
        cache.writes,
        cache.blob_count()
    );

    // 3. The second pass keeps the unit artifacts: the recorded digests
    //    still validate, and the cache sees only the two per-pass writes
    //    (manifest.toml and build-state.json rewrite every pass by design —
    //    the D-9 gate keeps the unit artifacts byte-for-byte).
    let second = session
        .emit_to(&mut cache, Profile::Layout, false)
        .expect("the warm pass emits");
    assert!(second.written.is_empty(), "digest-gated: nothing re-wrote");
    assert_eq!(
        cache.writes, 7,
        "only the per-pass manifest/state rewrite: {}",
        cache.writes
    );
    println!(
        "emit 2: kept {} artifact set(s) byte-for-byte — only manifest/state rewrite",
        second.kept.len()
    );

    // 4. An edit re-emits exactly that unit: five paths flip to the new
    //    digests, the unchanged-state blob is deduped where content
    //    repeats (manifest/state rewritten, ir/map/rs fresh).
    let edited = source.replace("Add", "Add2");
    assert_ne!(edited, source, "the edit is real");
    session.set_source("counter.tsx", edited);
    session.compile(Profile::Layout);
    let third = session
        .emit_to(&mut cache, Profile::Layout, false)
        .expect("the edited pass emits");
    println!(
        "emit 3: {} artifact set(s) re-wrote ({} sink writes total, {} deduped)",
        third.written.len(),
        cache.writes,
        cache.deduped
    );

    // 5. The seam's BR-1 gate: a denied unit refuses the WHOLE emission —
    //    the cache stays exactly as it was.
    let denied = concat!(
        "import { createSignal } from \"solid-js\";\n",
        "export function A() {\n",
        "  const [h, setH] = createSignal(\"<b>x</b>\");\n",
        "  return <div innerHTML={h()} />;\n",
        "}\n",
    );
    session.set_source("bad.tsx", denied);
    session.compile(Profile::Layout);
    let err = session
        .emit_to(&mut cache, Profile::Layout, false)
        .expect_err("a denied pass refuses the emission");
    println!(
        "emit 4: refused — {err}\n         help: {}",
        err.help().map(|h| h.to_string()).unwrap_or_default()
    );
    assert_eq!(cache.writes, 12, "the refusal wrote nothing");

    // The artifacts are in the cache, content-addressed: any path's bytes
    // resolve through its digest.
    println!(
        "\ncache://counter.rs: {} bytes (digest-keyed, dedupe-friendly)",
        cache.get("counter.rs").map_or(0, <[u8]>::len)
    );
}
