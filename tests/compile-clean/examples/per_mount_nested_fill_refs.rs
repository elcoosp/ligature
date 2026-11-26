//! The mount walk's recursive capture walk, walked end to end (M26 — the
//! M25 report's named follow-up): the committed `per_mount_nested_fill_ref`
//! unit — the same artifact the parity gate byte-pins on both frontend legs
//! — built on the mock host. A `ref` inside a NESTED outlet's fill of a
//! BRANCH-mounted use captures at mount time: the mount walk recurses into
//! the freshly mounted fill content (the M9 nested-outlet recursion), mounts
//! the nested fill through the guarded `Option<Panel>` chain, and the
//! PARENT's ref field takes the freshly mounted nested fill hole on every
//! mount — a remount cycle re-captures into a fresh node, never the disposed
//! one. Pre-M26 the shape denied at codegen (fail-closed) and the nested
//! fill never mounted at all — the bind arms addressed content that was
//! never inserted.
//!
//! Run without touching `dist/`:
//!
//! ```sh
//! cargo run -p compile-clean --example per_mount_nested_fill_refs
//! ```

use compile_clean::per_mount_nested_fill_ref;
use mock_host::MockHost;

fn main() {
    let mut host = MockHost::new();

    // 1. Build — `on` starts true, so the initial flush's mounting arm
    //    builds the Panel instance (the per-mount identity), mounts the
    //    header fill into its outlet, RECURSES into the fresh content (M26),
    //    mounts the nested `inner` fill into the header content's own
    //    outlet, and captures the ref from the freshly mounted NESTED fill.
    //    The build body never read the `Option` field.
    let mut app = per_mount_nested_fill_ref::App::build(
        &mut host,
        per_mount_nested_fill_ref::AppProps::default(),
    );
    let nested = app
        .panel
        .as_ref()
        .expect("instance built at mount")
        .frag
        .slot_fill(1)
        .and_then(|f| f.slot_fill(0))
        .map(|c| c.roots()[0])
        .expect("the nested fill mounted into the fresh instance's header content");
    println!(
        "mount 1: panel built, header fill mounted, nested fill mounted, \
         ref_mark = {} (the nested fill's root)",
        app.ref_mark.get()
    );
    assert_eq!(
        app.ref_mark, nested,
        "the recursive capture names the nested fill element"
    );

    // 2. The nested bind is live: the fill's text rides the composed chain
    //    (`slot_fill(1)` → `slot_fill(0)`) — pre-M26 this arm addressed
    //    content that was never inserted.
    app.set_tag("next".into());
    lig_runtime::flush(&mut app, &mut host);
    println!("flush: the nested fill's bind ran through the composed chain");

    // 3. Flip off — the unmount arm disposes the instance and everything
    //    mounted into it, nested fills included. The captured node is
    //    spent; the never-reused id discipline keeps the stale ref inert.
    app.set_on(false);
    lig_runtime::flush(&mut app, &mut host);
    assert!(app.panel.is_none(), "the instance disposed with the branch");
    println!(
        "mount 0: branch unmounted, instance disposed, ref_mark holds the spent id {}",
        app.ref_mark.get()
    );

    // 4. Flip on — a FRESH instance, header fill and NESTED fill mount, and
    //    the recursive capture re-runs inside the mounting arm: the ref
    //    takes the fresh nested element, never the disposed one.
    app.set_on(true);
    lig_runtime::flush(&mut app, &mut host);
    let fresh = app
        .panel
        .as_ref()
        .expect("fresh instance at remount")
        .frag
        .slot_fill(1)
        .and_then(|f| f.slot_fill(0))
        .map(|c| c.roots()[0])
        .expect("the nested fill remounted into the fresh instance");
    assert_ne!(fresh, nested, "the remount builds fresh nested nodes");
    assert_eq!(
        app.ref_mark, fresh,
        "the recursive capture re-ran on the fresh nested fill"
    );
    println!(
        "mount 2: fresh instance + nested fill, ref_mark = {} (the fresh nested root)",
        app.ref_mark.get()
    );
}
