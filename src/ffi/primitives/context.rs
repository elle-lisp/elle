//! Re-exports from crate::context for backward compatibility.

pub use crate::context::{
    clear_symbol_table, clear_vm_context, get_symbol_table, get_vm_context, resolve_symbol_name,
    set_symbol_table, set_vm_context,
};

use crate::vm::VM;

/// Register FFI primitives in the VM.
pub fn register_ffi_primitives(_vm: &mut VM) {
    // No-op: FFI primitives are registered via the PRIMITIVES table
}
