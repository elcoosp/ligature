//! The jump table (§2.4): one global `AtomicPtr<JumpTable>`; safepoint calls
//! look up the current function's address — hit → call the patched address,
//! miss → call the original. Application swaps the pointer atomically; no
//! text/GOT patching, ever.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicPtr, Ordering};

/// old runtime addr → new runtime addr (rebasing to file-relative happens at
/// manifest write time via the aslr reference).
#[derive(Debug, Default)]
pub struct JumpTable {
    map: BTreeMap<u64, u64>,
}

impl JumpTable {
    /// A table from an address map.
    #[must_use]
    pub fn new(map: BTreeMap<u64, u64>) -> Self {
        Self { map }
    }

    /// Look up the patched address for `old`.
    #[must_use]
    pub fn lookup(&self, old: u64) -> Option<u64> {
        self.map.get(&old).copied()
    }

    /// Number of mapped functions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether the table maps nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The map, for manifest serialization.
    #[must_use]
    pub fn as_map(&self) -> &BTreeMap<u64, u64> {
        &self.map
    }
}

static APP_JUMP_TABLE: AtomicPtr<JumpTable> = AtomicPtr::new(std::ptr::null_mut());

/// Read the currently-committed table (or None before the first patch).
#[must_use]
pub fn current_table() -> Option<&'static JumpTable> {
    let ptr = APP_JUMP_TABLE.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        // SAFETY: the pointer is only ever set to leaked tables that are
        // never freed (never-dlclose discipline, §2.4).
        Some(unsafe { &*ptr })
    }
}

/// Commit `table` atomically; returns the previous generation's mapping count.
///
/// # Panics
/// Never — allocation failure aborts, pointer swap is infallible.
pub fn commit_table(table: JumpTable) -> usize {
    let leaked = Box::leak(Box::new(table));
    let prev = APP_JUMP_TABLE.swap(leaked, Ordering::AcqRel);
    if prev.is_null() {
        0
    } else {
        // SAFETY: same leaked-table invariant as `current_table`.
        unsafe { (*prev).len() }
    }
}

/// Patch-apply failure.
#[derive(Debug)]
pub enum PatchError {
    /// The dylib could not be loaded.
    Load(libloading::Error),
    /// A requested symbol was missing from the dylib.
    MissingSymbol(String),
    /// The patchTip dylib did not change any requested symbol.
    NoChange,
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatchError::Load(e) => write!(f, "patch dylib load failed: {e}"),
            PatchError::MissingSymbol(s) => write!(f, "symbol not found in patch dylib: {s}"),
            PatchError::NoChange => write!(f, "patch changed none of the requested symbols"),
        }
    }
}

impl std::error::Error for PatchError {}

/// Well-known spike symbols (matched by mangled/exported name, §2.4).
pub const FRAME_LABEL: &str = "patchable_frame_label";
/// Second well-known spike symbol (multi-symbol map proof).
pub const FRAME_COUNT: &str = "patchable_frame_count";

/// The result of one patch application.
#[derive(Debug, Clone)]
pub struct PatchReceipt {
    /// How many symbols were remapped.
    pub mapped: usize,
    /// The dylib path (kept loaded forever).
    pub dylib: std::path::PathBuf,
    /// The ASLR reference read from the host process (`dlsym(RTLD_DEFAULT, "main")`).
    pub aslr_reference: u64,
    /// File-relative new addresses per requested symbol.
    pub file_relative: BTreeMap<String, u64>,
}

/// Load a patch dylib, resolve `symbols` against it, rebase against the ASLR
/// reference, and commit the new jump table atomically.
///
/// The dylib is **never `dlclose`d**: stale frames may still hold code inside
/// it; the memory is intentionally leaked (§2.4).
///
/// # Errors
/// [`PatchError::Load`] when the dylib cannot load; [`PatchError::MissingSymbol`]
/// when a requested symbol is absent; [`PatchError::NoChange`] when every new
/// address equals the old one (a no-op patch is a bug, not a success).
///
/// # Panics
/// If `dlsym(RTLD_DEFAULT, "main")` fails — the ASLR anchor is load-bearing.
pub fn apply_patch(dylib: &std::path::Path, symbols: &[&str]) -> Result<PatchReceipt, PatchError> {
    // Never-dlclose: leak the Library.
    // SAFETY: the dylib path was produced by our own build step (patch-spike /
    // patch driver); loading it is the documented purpose of this module and
    // the loaded code is exercised only through the typed safepoint call.
    let lib: &'static libloading::Library = Box::leak(Box::new(
        unsafe { libloading::Library::new(dylib) }.map_err(PatchError::Load)?,
    ));

    // Old addresses from the running (fat) binary: dlsym(RTLD_DEFAULT, …)
    // searches the global scope, which includes the host executable when it
    // exports the patchable symbols (linked with -rdynamic on Linux).
    let mut map = BTreeMap::new();
    let mut file_relative = BTreeMap::new();
    for sym in symbols {
        let c = std::ffi::CString::new(*sym).expect("symbol names are c-strings");
        let new_addr: libloading::Symbol<*const ()> = unsafe { lib.get(c.as_bytes_with_nul()) }
            .map_err(|_| PatchError::MissingSymbol((*sym).to_string()))?;
        let new_addr = *new_addr as u64;
        let old_addr = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c.as_ptr()) };
        let old_addr = old_addr as usize as u64;
        if new_addr == old_addr {
            continue;
        }
        if old_addr == 0 {
            // symbol only exists in the patch (new function) — map by its new
            // address directly so first calls route to the patch.
            map.insert(new_addr, new_addr);
        } else {
            map.insert(old_addr, new_addr);
        }
        file_relative.insert((*sym).to_string(), file_relative_addr(new_addr));
    }
    if map.is_empty() {
        return Err(PatchError::NoChange);
    }

    let aslr_reference = aslr_reference();
    let mapped = map.len();
    commit_table(JumpTable::new(map));

    Ok(PatchReceipt {
        mapped,
        dylib: dylib.to_path_buf(),
        aslr_reference,
        file_relative,
    })
}

/// The ASLR anchor: address of `main` via `dlsym(RTLD_DEFAULT, "main")`.
///
/// # Panics
/// When `main` cannot be resolved — hosts require it (subsecond's anchor).
#[must_use]
pub fn aslr_reference() -> u64 {
    let main = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"main".as_ptr()) };
    assert!(!main.is_null(), "ASLR anchor `main` not resolvable (dlsym)");
    main as u64
}

/// File-relative address of a runtime address, using `dladdr` on the symbol's
/// own segment base (the protocol's rebase, §11.3 step 3).
#[must_use]
pub fn file_relative_addr(addr: u64) -> u64 {
    let mut info = libc::Dl_info {
        dli_fname: std::ptr::null(),
        dli_fbase: std::ptr::null_mut(),
        dli_sname: std::ptr::null(),
        dli_saddr: std::ptr::null_mut(),
    };
    let found = unsafe {
        libc::dladdr(
            addr as *const core::ffi::c_void,
            std::ptr::from_mut(&mut info),
        )
    };
    if found != 0 && !info.dli_fbase.is_null() {
        addr.saturating_sub(info.dli_fbase as u64)
    } else {
        addr
    }
}

/// Safepoint call (§2.4 `subsecond::call`): route `f` through the jump table;
/// a hit calls the patched address, a miss calls the original.
///
/// # Panics
/// Never for well-formed function pointers.
#[must_use]
pub fn hot_call<R>(f: fn() -> R) -> R {
    match current_table().and_then(|t| t.lookup(f as usize as u64)) {
        Some(new_addr) if new_addr != f as usize as u64 => {
            // SAFETY: the target is a function in a loaded (never-unmapped)
            // dylib with the same signature, matched by symbol name.
            let patched: fn() -> R = unsafe { core::mem::transmute(new_addr) };
            patched()
        }
        _ => f(),
    }
}
