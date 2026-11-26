//! The component registry walkthrough: two generated counter units — the
//! same artifact `lig reproducibility` byte-pins — mounted into ONE
//! `ComponentRegistry` on the mock host and driven through the trait
//! object, per id and as a scene.
//!
//! The mount seam's report named the missing piece this example closes:
//! "A `dyn` component registry (host stores mounted components behind
//! `&mut dyn ComponentUnit`) has the trait ready but no shipped container".
//! [`Mounted`](lig_runtime::Mounted) is the per-component front; the
//! registry is the host-side store for scenes of components — ids handed
//! out per mount, every tick the checked front (`flush_checked`, the
//! typed `CycleError`, LIG-E401), every disposal the total LIFO teardown.
//! What the trait object drives is the reaction mark + the checked tick —
//! the exact body the generated `fire_N` glue emits after its event
//! capture; setters stay inherent on the concrete state (the `Mounted`
//! front, reached through a host side-table keyed by the same ids).
//!
//! Run without touching `dist/`:
//!
//! ```sh
//! cargo run -p compile-clean --example component_registry
//! ```

use compile_clean::counter::{self, Counter};
use lig_runtime::{ComponentRegistry, ComponentUnit};
use mock_host::MockHost;

fn main() {
    // 1. The style table: both counters intern the same static style; the
    //    host binds the table once and resolves every `set_style` commit.
    let mut host = MockHost::new();
    host.bind_style_table(counter::generated::Counter_STYLE_CSS);

    // 2. Mount a scene of two counters into the registry — each mount runs
    //    the generated build (its own init flush included) and hands back a
    //    fresh `CompId` (`c0`, `c1`: ids are never reused after disposal).
    let mut registry = ComponentRegistry::new();
    let left = registry.mount(&mut host, |h| {
        Counter::build(h, counter::generated::CounterProps {})
    });
    let right = registry.mount(&mut host, |h| {
        Counter::build(h, counter::generated::CounterProps {})
    });
    println!(
        "mounted {left} and {right} — registry holds {}",
        registry.len()
    );
    assert_eq!(text_writes(&host, "0"), 2, "both init flushes rendered 0");

    // 3. One save-tick per id through the trait object: mark the handler
    //    reaction (the bit the generated `fire_1` glue marks: the click
    //    effect), then the checked flush — one call each, per component.
    for id in [left, right] {
        registry
            .update(id, &mut host, |s: &mut dyn ComponentUnit| {
                s.fragment().mark_mask(1 << 1);
            })
            .expect("the id is mounted")
            .expect("the counter converges");
    }
    assert_eq!(
        text_writes(&host, "1"),
        2,
        "each tick incremented its counter"
    );

    // 4. The scene tick: mark one component through `get` (the trait-reachable
    //    reaction surface), then drain every mounted component in one
    //    `flush_all` — the calls a host loop repeats per frame.
    registry
        .get(right)
        .expect("mounted")
        .fragment()
        .mark_mask(1 << 1);
    registry.flush_all(&mut host).expect("the scene converges");
    assert_eq!(
        text_writes(&host, "2"),
        1,
        "only the marked counter moved again"
    );

    // 5. The scene teardown: every node every mounted build created reaches
    //    the host as one `destroy` commit, exactly once — and the registry
    //    empties.
    let creates = host.count_kind("Create");
    registry.dispose_all(&mut host);
    let destroys = host.count_kind("Destroy");
    println!(
        "disposed the scene: {creates} create commits, {destroys} destroy commits — balanced: {}",
        creates == destroys
    );
    assert!(registry.is_empty(), "dispose_all empties the registry");
    assert_eq!(creates, destroys, "total disposal balances the stream");
}

/// How many `set_text` commits carried this exact text — the rendered
/// counter value, per initial flush / per tick.
fn text_writes(host: &MockHost, text: &str) -> usize {
    host.commits
        .iter()
        .filter(|c| matches!(c, mock_host::Commit::SetText { text: t, .. } if t == text))
        .count()
}
