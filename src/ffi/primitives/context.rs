//! VM context management for FFI primitives.
//!
//! Provides thread-local storage and management of the current VM context.

use crate::vm::VM;
use std::cell::RefCell;

thread_local! {
    static VM_CONTEXT: RefCell<Option<*mut VM>> = const { RefCell::new(None) };
}

/// Set the current VM context (called before executing code)
pub fn set_vm_context(vm: *mut VM) {
    VM_CONTEXT.with(|ctx| *ctx.borrow_mut() = Some(vm));
}

/// Get the current VM context
pub fn get_vm_context() -> Option<*mut VM> {
    VM_CONTEXT.with(|ctx| ctx.borrow().as_ref().copied())
}

/// Clear the VM context
pub fn clear_vm_context() {
    VM_CONTEXT.with(|ctx| *ctx.borrow_mut() = None);
}

/// Register FFI primitives in the VM.
pub fn register_ffi_primitives(_vm: &mut VM) {
    // Phase 2: FFI primitives for function calling
    // Note: These are meant to be called from Elle code
}
