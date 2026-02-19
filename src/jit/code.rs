//! JIT-compiled code wrapper
//!
//! This module provides the `JitCode` type that wraps a native function pointer
//! and keeps the JIT module alive to prevent the code from being freed.

use std::sync::Arc;

/// Wrapper to make JITModule Send + Sync
///
/// # Safety
/// The JITModule contains raw pointers to executable code. Once finalized,
/// the code is immutable and can be safely shared between threads.
/// The module itself should not be modified after finalization.
struct ModuleHolder(#[allow(dead_code)] cranelift_jit::JITModule);

// Safety: After finalization, the JITModule only contains immutable code.
// The raw pointers point to executable memory that doesn't change.
unsafe impl Send for ModuleHolder {}
unsafe impl Sync for ModuleHolder {}

/// Compiled native code for a function
///
/// This type wraps a native function pointer and keeps the JIT module alive
/// so the code isn't freed while still in use.
pub struct JitCode {
    /// The native function pointer
    fn_ptr: *const u8,
    /// Keep the module alive so the code isn't freed
    _module: Arc<ModuleHolder>,
}

// Safety: The function pointer points to immutable code that doesn't
// reference any thread-local state. The module is kept alive by Arc.
unsafe impl Send for JitCode {}
unsafe impl Sync for JitCode {}

impl JitCode {
    /// Create a new JitCode from a function pointer and module
    pub(crate) fn new(fn_ptr: *const u8, module: cranelift_jit::JITModule) -> Self {
        JitCode {
            fn_ptr,
            _module: Arc::new(ModuleHolder(module)),
        }
    }

    /// Get the native function pointer
    pub fn fn_ptr(&self) -> *const u8 {
        self.fn_ptr
    }

    /// Call the JIT-compiled function
    ///
    /// # Safety
    /// - `env` must point to a valid array of `Value` with at least as many
    ///   elements as the function expects captures
    /// - `args` must point to a valid array of `Value` with at least `nargs` elements
    /// - `globals` must be a valid pointer to the VM globals structure
    #[inline]
    pub unsafe fn call(
        &self,
        env: *const u64,
        args: *const u64,
        nargs: u32,
        globals: *mut (),
    ) -> u64 {
        let f: unsafe extern "C" fn(*const u64, *const u64, u32, *mut ()) -> u64 =
            std::mem::transmute(self.fn_ptr);
        f(env, args, nargs, globals)
    }
}

impl std::fmt::Debug for JitCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JitCode")
            .field("fn_ptr", &self.fn_ptr)
            .finish()
    }
}
