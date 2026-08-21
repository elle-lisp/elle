//! Elle Foreign Function Interface (FFI) subsystem.
//!
//! Enables calling C functions from Elle code. This module is being rebuilt
//! to match the design in docs/ffi.md.
//!
//! The `ffi` cargo feature gates the libffi-dependent call/callback
//! machinery. Type descriptors and library loading are always available.

#[cfg(feature = "ffi")]
pub mod call;
#[cfg(feature = "ffi")]
pub mod callback;
#[cfg(feature = "ffi")]
pub(crate) mod from_c;
#[cfg(feature = "ffi")]
pub mod marshal;
pub mod registry;
#[cfg(feature = "ffi")]
pub(crate) mod to_c;
pub mod types;

#[cfg(feature = "ffi")]
use callback::CallbackStore;
use std::collections::HashMap;
use std::path::PathBuf;

/// The FFI subsystem manages this VM's loaded-library *ids* and active callbacks.
///
/// The actual library mappings live process-globally in [`registry`] (never
/// unloaded — see that module's docs); this per-VM table maps the small per-VM ids
/// the `HeapObject::LibHandle(u32)` value carries to their registry keys. It owns no
/// `libloading::Library`, so dropping a worker's `FFISubsystem` on teardown
/// `dlclose`s nothing — the worker-teardown TSD-destructor crash is impossible by
/// construction.
pub(crate) struct FFISubsystem {
    /// Loaded libraries: per-VM id -> process-global registry key (canonical path).
    libraries: HashMap<u32, PathBuf>,
    /// Next library ID to assign
    next_lib_id: u32,
    /// Active FFI callbacks: code_ptr -> ActiveCallback
    #[cfg(feature = "ffi")]
    callbacks: CallbackStore,
    /// Error from the most recent callback invocation on this VM, if any. The
    /// trampoline (`callback::trampoline_callback`) stores a signalling closure's
    /// error here; `ffi/call` drains it after the C function returns. Single-VM by
    /// the documented callback limitation, so a `FFISubsystem` field is the right
    /// scope.
    #[cfg(feature = "ffi")]
    callback_error: Option<crate::value::Value>,
}

impl FFISubsystem {
    /// Create a new FFI subsystem.
    pub fn new() -> Self {
        FFISubsystem {
            libraries: HashMap::new(),
            next_lib_id: 1,
            #[cfg(feature = "ffi")]
            callbacks: CallbackStore::new(),
            #[cfg(feature = "ffi")]
            callback_error: None,
        }
    }

    /// Load a shared library into the process-global registry and mint a per-VM id
    /// for it. The mapping is process-lifetime (never `dlclose`d); this only records
    /// the id → registry-key mapping for symbol lookup.
    pub fn load_library(&mut self, path: &str) -> Result<u32, String> {
        let key = registry::load(path)?;
        let id = self.next_lib_id;
        self.next_lib_id += 1;
        self.libraries.insert(id, key);
        Ok(id)
    }

    /// Load the current process as a library (dlopen(NULL)).
    pub fn load_self(&mut self) -> Result<u32, String> {
        let key = registry::load_self()?;
        let id = self.next_lib_id;
        self.next_lib_id += 1;
        self.libraries.insert(id, key);
        Ok(id)
    }

    /// Resolve a symbol in a loaded library by per-VM id, returning a raw pointer
    /// (valid for the process lifetime — the mapping is never unloaded). Collapses
    /// the old `get_library(id)` → `LibraryHandle::get_symbol` two-hop and holds no
    /// borrow into the registry.
    pub fn get_symbol(&self, id: u32, sym: &str) -> Result<*const std::ffi::c_void, String> {
        let key = self
            .libraries
            .get(&id)
            .ok_or_else(|| format!("library {} not loaded", id))?;
        registry::symbol(key, sym)
    }

    /// Register an ordered teardown (a zero-arg C symbol) for a loaded library —
    /// the backend of `ffi/on-unload`. Explicit-only: run by `ffi/run-teardowns`,
    /// never by the runtime (see [`registry`] docs).
    pub fn register_teardown(&self, id: u32, sym: &str) -> Result<(), String> {
        let key = self
            .libraries
            .get(&id)
            .ok_or_else(|| format!("library {} not loaded", id))?;
        registry::register_teardown(key, sym)
    }

    /// Get mutable access to the callback store.
    #[cfg(feature = "ffi")]
    pub fn callbacks_mut(&mut self) -> &mut CallbackStore {
        &mut self.callbacks
    }

    /// Stash an error raised by a callback invocation (set by the trampoline).
    #[cfg(feature = "ffi")]
    pub fn set_callback_error(&mut self, err: crate::value::Value) {
        self.callback_error = Some(err);
    }

    /// Take the pending callback error, if any (drained by `ffi/call`).
    #[cfg(feature = "ffi")]
    pub fn take_callback_error(&mut self) -> Option<crate::value::Value> {
        self.callback_error.take()
    }
}

impl Default for FFISubsystem {
    fn default() -> Self {
        Self::new()
    }
}
