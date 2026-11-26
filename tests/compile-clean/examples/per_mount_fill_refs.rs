//! The per-mount fill refs, walked end to end (M25 — the M18 gap list's
//! ref-composed guard): the committed `per_mount_fill_ref` unit — the same
//! artifact the parity gate byte-pins on both frontend legs — built on the
//! mock host. A `ref` inside the fill of a BRANCH-mounted use captures at
//! mount time: the build body has no instance to read (the field is the
//! per-mount `Option<Panel>`), so the mounting arm's guarded walk takes the
//! freshly mounted fill hole on every mount — and a remount cycle re-captures
//! into a fresh node, never the disposed one.
//!
//! Run without touching `dist/`:
//!
//! ```sh
//! cargo run -p compile-clean --example per_mount_fill_refs
//! ```

use compile_clean::per_mount_fill_ref;
use mock_host::MockHost;

fn main() {
    let mut host = MockHost::new();

    // 1. Build — `on` starts true, so the initial flush's mounting arm
    //    builds the Panel instance (the per-mount identity), mounts the
    //    fill content into its `header` outlet, and captures the ref from
    //    the freshly mounted content. The build body never read the
    //    `Option` field: the pre-M18 spelling could not even compile.
    let mut app =
        per_mount_fill_ref::App::build(&mut host, per_mount_fill_ref::AppProps::default());
    let panel = app.panel.as_ref().expect("instance built at mount");
    let first = panel
        .frag
        .slot_fill(1)
        .map(|c| c.roots()[0])
        .expect("the fill mounted into the fresh instance");
    println!(
        "mount 1: panel built, fill mounted, ref_mark = {}",
        app.ref_mark.get()
    );
    assert_eq!(app.ref_mark, first, "the capture names the fill element");

    // 2. Flip off — the unmount arm disposes the instance and its fills.
    //    The captured node is spent; the never-reused id discipline keeps
    //    the stale ref inert (it can never name newer state).
    app.set_on(false);
    lig_runtime::flush(&mut app, &mut host);
    assert!(app.panel.is_none(), "the instance disposed with the branch");
    println!(
        "mount 0: branch unmounted, instance disposed, ref_mark holds the spent id {}",
        app.ref_mark.get()
    );

    // 3. Flip on — a FRESH instance and fill mount, and the mount-time
    //    capture re-runs inside the mounting arm: the ref takes the fresh
    //    element, never the disposed one.
    app.set_on(true);
    lig_runtime::flush(&mut app, &mut host);
    let panel = app.panel.as_ref().expect("fresh instance at remount");
    let second = panel
        .frag
        .slot_fill(1)
        .map(|c| c.roots()[0])
        .expect("the fill remounted into the fresh instance");
    assert_ne!(second, first, "the remount builds fresh nodes");
    assert_eq!(app.ref_mark, second, "the capture re-ran per mount");
    println!(
        "mount 2: fresh fill mounted, ref_mark re-captured = {} (was {})",
        app.ref_mark.get(),
        first.get()
    );
    println!("per-mount fill refs: capture rides every mount, dispose rides every unmount");
}
