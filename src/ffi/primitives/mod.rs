//! FFI primitive functions for Elle.

pub mod context;

pub use context::{
    clear_vm_context, get_vm_context, register_ffi_primitives, set_symbol_table, set_vm_context,
};
