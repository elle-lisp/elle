//! Elle Foreign Function Interface (FFI) subsystem.
//!
//! Enables calling C/C++ functions from Elle Lisp code.
//!
//! # Example
//!
//! ```lisp
//! ; Load a C library and call functions from Elle
//! (load-library "/lib/x86_64-linux-gnu/libc.so.6")
//! (strlen "hello")  ; => 5
//! ```

pub mod bindings;
pub mod call;
pub mod callback;
pub mod handler_marshal;
pub mod handlers;
pub mod header;
pub mod loader;
pub mod marshal;
pub mod memory;
pub mod primitives;
pub mod safety;
pub mod symbol;
pub mod types;
pub mod wasm;

use handlers::HandlerRegistry;
use loader::LibraryHandle;
use std::collections::HashMap;
use symbol::SymbolResolver;
use types::{StructId, StructLayout};

/// The FFI subsystem manages loaded libraries and cached symbols.
pub struct FFISubsystem {
    /// Loaded libraries: id -> handle
    libraries: HashMap<u32, LibraryHandle>,
    /// Next library ID to assign
    next_lib_id: u32,
    /// Symbol resolver with caching
    symbol_resolver: SymbolResolver,
    /// Registered struct layouts: id -> layout
    struct_layouts: HashMap<u32, StructLayout>,
    /// Next struct ID to assign
    next_struct_id: u32,
    /// Custom type handler registry
    handler_registry: HandlerRegistry,
}

impl FFISubsystem {
    /// Create a new FFI subsystem.
    pub fn new() -> Self {
        FFISubsystem {
            libraries: HashMap::new(),
            next_lib_id: 1,
            symbol_resolver: SymbolResolver::new(),
            struct_layouts: HashMap::new(),
            next_struct_id: 1,
            handler_registry: HandlerRegistry::new(),
        }
    }

    /// Load a shared library.
    ///
    /// # Arguments
    /// * `path` - Path to library file (.so on Linux)
    ///
    /// # Returns
    /// * `Ok(id)` - Library ID for future reference
    /// * `Err(message)` - If loading fails
    pub fn load_library(&mut self, path: &str) -> Result<u32, String> {
        let mut lib = loader::load_library(path)?;
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

    /// Get a mutable reference to a loaded library.
    pub fn get_library_mut(&mut self, id: u32) -> Option<&mut LibraryHandle> {
        self.libraries.get_mut(&id)
    }

    /// Unload a library (remove from registry).
    pub fn unload_library(&mut self, id: u32) -> Option<LibraryHandle> {
        self.libraries.remove(&id)
    }

    /// Get the symbol resolver.
    pub fn symbol_resolver(&mut self) -> &mut SymbolResolver {
        &mut self.symbol_resolver
    }

    /// List all loaded libraries.
    pub fn loaded_libraries(&self) -> Vec<(u32, String)> {
        self.libraries
            .iter()
            .map(|(id, lib)| (*id, lib.path.clone()))
            .collect()
    }

    /// Register a struct layout.
    pub fn register_struct_layout(&mut self, layout: StructLayout) -> StructId {
        let id = StructId::new(self.next_struct_id);
        self.next_struct_id += 1;
        self.struct_layouts.insert(id.0, layout);
        id
    }

    /// Get a registered struct layout by ID.
    pub fn get_struct_layout(&self, id: StructId) -> Option<&StructLayout> {
        self.struct_layouts.get(&id.0)
    }

    /// Get a mutable reference to a registered struct layout.
    pub fn get_struct_layout_mut(&mut self, id: StructId) -> Option<&mut StructLayout> {
        self.struct_layouts.get_mut(&id.0)
    }

    /// List all registered struct layouts.
    pub fn struct_layouts(&self) -> Vec<(StructId, &StructLayout)> {
        self.struct_layouts
            .iter()
            .map(|(id, layout)| (StructId(*id), layout))
            .collect()
    }

    /// Get the custom type handler registry.
    pub fn handler_registry(&self) -> &HandlerRegistry {
        &self.handler_registry
    }

    /// Marshal an Elle value to C with handler support.
    ///
    /// This method checks for registered custom handlers before falling back
    /// to default marshaling.
    pub fn marshall_elle_to_c(
        &self,
        value: &crate::value::Value,
        ctype: &types::CType,
    ) -> Result<marshal::CValue, String> {
        handler_marshal::HandlerMarshal::elle_to_c_with_handlers(
            value,
            ctype,
            &self.handler_registry,
        )
    }

    /// Unmarshal a C value to Elle with handler support.
    ///
    /// This method checks for registered custom handlers before falling back
    /// to default unmarshaling.
    pub fn marshall_c_to_elle(
        &self,
        cval: &marshal::CValue,
        ctype: &types::CType,
    ) -> Result<crate::value::Value, String> {
        handler_marshal::HandlerMarshal::c_to_elle_with_handlers(
            cval,
            ctype,
            &self.handler_registry,
        )
    }
}

impl Default for FFISubsystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffi_subsystem_creation() {
        let ffi = FFISubsystem::new();
        assert_eq!(ffi.loaded_libraries().len(), 0);
    }

    #[test]
    fn test_ffi_subsystem_default() {
        let ffi = FFISubsystem::default();
        assert_eq!(ffi.loaded_libraries().len(), 0);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_load_library() {
        let mut ffi = FFISubsystem::new();
        let result = ffi
            .load_library("/lib/x86_64-linux-gnu/libc.so.6")
            .or_else(|_| ffi.load_library("/lib64/libc.so.6"))
            .or_else(|_| ffi.load_library("libc.so.6"));

        if let Ok(id) = result {
            assert!(ffi.get_library(id).is_some());
            assert_eq!(ffi.loaded_libraries().len(), 1);
        }
    }

    #[test]
    fn test_unload_library() {
        let mut ffi = FFISubsystem::new();
        let id = 42;

        // Try to unload nonexistent library
        assert!(ffi.unload_library(id).is_none());
    }

    #[test]
    fn test_struct_layout_registration() {
        use crate::ffi::types::{StructField, StructId, StructLayout};

        let mut ffi = FFISubsystem::new();

        // Create a struct layout
        let layout = StructLayout::new(
            StructId::new(1),
            "Point".to_string(),
            vec![
                StructField {
                    name: "x".to_string(),
                    ctype: types::CType::Int,
                    offset: 0,
                },
                StructField {
                    name: "y".to_string(),
                    ctype: types::CType::Int,
                    offset: 4,
                },
            ],
            8,
            4,
        );

        // Register it
        let id = ffi.register_struct_layout(layout.clone());

        // Retrieve it
        let retrieved = ffi.get_struct_layout(id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "Point");
        assert_eq!(retrieved.unwrap().size, 8);
    }

    #[test]
    fn test_multiple_struct_layouts() {
        use crate::ffi::types::{StructId, StructLayout};

        let mut ffi = FFISubsystem::new();

        // Register multiple struct layouts
        let layout1 = StructLayout::new(StructId::new(1), "Point".to_string(), vec![], 8, 4);

        let layout2 = StructLayout::new(StructId::new(2), "Rectangle".to_string(), vec![], 16, 4);

        let id1 = ffi.register_struct_layout(layout1);
        let id2 = ffi.register_struct_layout(layout2);

        // Verify both are registered
        assert_ne!(id1.0, id2.0);
        assert!(ffi.get_struct_layout(id1).is_some());
        assert!(ffi.get_struct_layout(id2).is_some());

        // List them
        let layouts = ffi.struct_layouts();
        assert_eq!(layouts.len(), 2);
    }
}
