//! S2 spike tip crate: the "application code" that gets hot-patched.
//!
//! v0 is baked into the dummy-host test binary; each "edit" rebuilds this crate
//! as a cdylib with `LIG_TIP_LABEL`/`LIG_TIP_COUNT` set, and `lig-patch` swaps
//! the jump table so the next safepoint call lands in the new build. M3's
//! two-generation spike applies v1 then v2 to the same running host; the
//! `patchable_frame_extra` symbol is the **stub thunk** proof: the fat host
//! exports a placeholder body, generation 2 fills it (ADR-013, "the full
//! thunk story lands with M3").

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

/// The stub-thunk target: the fat host exports a placeholder under this name;
/// a patch generation fills it with the real body (M3 ThinLink story).
#[unsafe(no_mangle)]
pub extern "Rust" fn patchable_frame_extra() -> u64 {
    match option_env!("LIG_TIP_EXTRA") {
        Some(c) => c.parse().unwrap_or(7),
        None => 7,
    }
}
