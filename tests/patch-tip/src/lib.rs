//! S2 spike tip crate: the "application code" that gets hot-patched.
//!
//! v0 is baked into the dummy-host test binary; each "edit" rebuilds this crate
//! as a cdylib with `LIG_TIP_LABEL` set, and `lig-patch` swaps the jump table so
//! the next safepoint call lands in the new build.

/// Label reported through the patchable safepoint.
#[unsafe(no_mangle)]
pub static LIG_TIP_LABEL_V1: &str = match option_env!("LIG_TIP_LABEL") {
    Some(l) => l,
    None => "v0",
};

/// Patchable function — resolved by symbol name, like subsecond thunks (§2.4).
#[unsafe(no_mangle)]
pub extern "Rust" fn patchable_frame_label() -> &'static str {
    LIG_TIP_LABEL_V1
}

/// Second patchable function to prove multi-symbol maps.
#[unsafe(no_mangle)]
pub extern "Rust" fn patchable_frame_count() -> u64 {
    match option_env!("LIG_TIP_COUNT") {
        Some(c) => c.parse().unwrap_or(0),
        None => 0,
    }
}
