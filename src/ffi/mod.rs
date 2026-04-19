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
pub mod loader;
#[cfg(feature = "ffi")]
pub mod marshal;
pub mod primitives;
#[cfg(feature = "ffi")]
pub(crate) mod to_c;
pub mod types;

#[cfg(feature = "ffi")]
use callback::CallbackStore;
use loader::LibraryHandle;
use std::collections::HashMap;

/// The FFI subsystem manages loaded libraries and active callbacks.
pub(crate) struct FFISubsystem {
    /// Loaded libraries: id -> handle
    libraries: HashMap<u32, LibraryHandle>,
    /// Next library ID to assign
    next_lib_id: u32,
    /// Active FFI callbacks: code_ptr -> ActiveCallback
    #[cfg(feature = "ffi")]
    callbacks: CallbackStore,
}

impl FFISubsystem {
    /// Create a new FFI subsystem.
    pub fn new() -> Self {
        FFISubsystem {
            libraries: HashMap::new(),
            next_lib_id: 1,
            #[cfg(feature = "ffi")]
            callbacks: CallbackStore::new(),
        }
    }

    /// Load a shared library.
    pub fn load_library(&mut self, path: &str) -> Result<u32, String> {
        let mut lib = loader::load_library(path)?;
        let id = self.next_lib_id;
        lib.id = id;
        self.next_lib_id += 1;
        self.libraries.insert(id, lib);
        Ok(id)
    }

    /// Load the current process as a library (dlopen(NULL)).
    pub fn load_self(&mut self) -> Result<u32, String> {
        let mut lib = loader::load_self()?;
        let id = self.next_lib_id;
        lib.id = id;
        self.next_lib_id += 1;
        self.libraries.insert(id, lib);
        Ok(id)
    }

    /// Get a loaded library by ID.
    pub fn get_library(&self, id: u32) -> Option<&LibraryHandle> {
        self.libraries.get(&id)
    }

    /// Get mutable access to the callback store.
    #[cfg(feature = "ffi")]
    pub fn callbacks_mut(&mut self) -> &mut CallbackStore {
        &mut self.callbacks
    }
}

impl Default for FFISubsystem {
    fn default() -> Self {
        Self::new()
    }
}
