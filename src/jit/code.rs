//! JIT-compiled code wrapper
//!
//! This module provides the `JitCode` type that wraps a native function pointer
//! and keeps the JIT module alive to prevent the code from being freed.

use std::sync::Arc;

use crate::jit::value::JitValue;
use crate::value::Value;

/// Wrapper to make JITModule Send + Sync
///
/// # Safety
/// The JITModule contains raw pointers to executable code. Once finalized,
/// the code is immutable and can be safely shared between threads.
/// The module itself should not be modified after finalization.
pub(crate) struct ModuleHolder(#[allow(dead_code)] cranelift_jit::JITModule);

impl ModuleHolder {
    pub(crate) fn new(module: cranelift_jit::JITModule) -> Self {
        ModuleHolder(module)
    }
}

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
    /// Yield point metadata for side-exit support.
    /// Indexed by yield point index (u32 immediate in JIT code).
    /// Empty for non-yielding functions.
    /// Read by `elle_jit_yield` runtime helper.
    #[allow(dead_code)]
    pub(crate) yield_points: Vec<super::dispatch::YieldPointMeta>,
    /// Call site metadata for yield-through-call support.
    /// Indexed by call site index (u32 immediate in JIT code).
    /// Empty for non-yielding functions.
    /// Read by `elle_jit_yield_through_call` runtime helper.
    #[allow(dead_code)]
    pub(crate) call_sites: Vec<super::dispatch::CallSiteMeta>,
    /// Nested-lambda template **blueprints** referenced by MakeClosure
    /// instructions. The native code bakes a raw pointer to each, so each must
    /// outlive the JIT code and keep a stable address (hence `Box`). This is
    /// template *data* (the code object's blueprints), not a region value —
    /// `elle_jit_make_closure` re-materializes a fresh region-allocated template
    /// per execution.
    #[allow(dead_code, clippy::vec_box)]
    pub(crate) closure_protos: Vec<std::rc::Rc<crate::value::TemplateProto>>,
    /// Immutable heap-literal templates baked by `MaterializeConst` (a string, or
    /// a quoted compound structure). The native code holds a raw pointer to each
    /// `ConstTemplate`, so they must live as long as the JIT code itself (this
    /// code object owns them). Each `Box` has a stable heap address. This is
    /// template *data* (the code object's constant pool), not a region value —
    /// re-materialized per execution.
    //
    // clippy::vec_box: the `Box` is load-bearing, not redundant indirection —
    // native code bakes a raw pointer to each `ConstTemplate`, so an element must
    // not move when the Vec grows; `Box` gives each a stable address.
    #[allow(dead_code, clippy::vec_box)]
    pub(crate) templates: Vec<Box<crate::value::ConstTemplate>>,
}

// Safety: The function pointer points to immutable code that doesn't
// reference any thread-local state. The module is kept alive by Arc.
unsafe impl Send for JitCode {}
unsafe impl Sync for JitCode {}

impl JitCode {
    /// Create a new JitCode from a function pointer and module
    #[allow(dead_code)]
    pub(crate) fn new(fn_ptr: *const u8, module: cranelift_jit::JITModule) -> Self {
        JitCode {
            fn_ptr,
            _module: Arc::new(ModuleHolder::new(module)),
            yield_points: Vec::new(),
            call_sites: Vec::new(),
            closure_protos: Vec::new(),
            templates: Vec::new(),
        }
    }

    /// Create a new JitCode from a function pointer and a shared module
    ///
    /// This constructor is used for batch compilation where multiple JitCode
    /// instances share one module. Closure template blueprints and
    /// string-literal templates must be passed in to keep them alive for the
    /// JitCode's lifetime.
    #[allow(clippy::vec_box)] // stable per-element address for baked JIT pointers
    pub(crate) fn new_shared(
        fn_ptr: *const u8,
        module: Arc<ModuleHolder>,
        closure_protos: Vec<std::rc::Rc<crate::value::TemplateProto>>,
        templates: Vec<Box<crate::value::ConstTemplate>>,
    ) -> Self {
        JitCode {
            fn_ptr,
            _module: module,
            yield_points: Vec::new(),
            call_sites: Vec::new(),
            closure_protos,
            templates,
        }
    }

    /// Create a new JitCode with yield point, call site metadata, closure
    /// template blueprints, and string-literal templates.
    #[allow(clippy::vec_box)] // stable per-element address for baked JIT pointers
    pub(crate) fn new_with_metadata(
        fn_ptr: *const u8,
        module: cranelift_jit::JITModule,
        yield_points: Vec<super::dispatch::YieldPointMeta>,
        call_sites: Vec<super::dispatch::CallSiteMeta>,
        closure_protos: Vec<std::rc::Rc<crate::value::TemplateProto>>,
        templates: Vec<Box<crate::value::ConstTemplate>>,
    ) -> Self {
        JitCode {
            fn_ptr,
            _module: Arc::new(ModuleHolder::new(module)),
            yield_points,
            call_sites,
            closure_protos,
            templates,
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
    /// - `vm` must be a valid pointer to the VM struct
    /// - `self_tag`/`self_payload` are the tag and payload of the closure being
    ///   executed (for self-tail-call detection)
    #[inline]
    pub unsafe fn call(
        &self,
        env: *const Value,
        args: *const Value,
        nargs: u32,
        vm: *mut (),
        self_tag: u64,
        self_payload: u64,
    ) -> JitValue {
        let f: unsafe extern "C" fn(
            *const Value,
            *const Value,
            u32,
            *mut (),
            u64,
            u64,
        ) -> JitValue = std::mem::transmute(self.fn_ptr);
        f(env, args, nargs, vm, self_tag, self_payload)
    }
}

#[cfg(test)]
impl JitCode {
    /// Create a JitCode with yield points but no real compiled code.
    /// For testing `elle_jit_yield` without Cranelift compilation.
    #[allow(dead_code)]
    pub(crate) fn test_with_yield_points(
        yield_points: Vec<super::dispatch::YieldPointMeta>,
    ) -> Self {
        use cranelift_jit::{JITBuilder, JITModule};
        let flag_builder = cranelift_codegen::settings::builder();
        let isa_builder = cranelift_native::builder().unwrap();
        let isa = isa_builder
            .finish(cranelift_codegen::settings::Flags::new(flag_builder))
            .unwrap();
        let builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        let module = JITModule::new(builder);
        JitCode {
            fn_ptr: std::ptr::null(),
            _module: Arc::new(ModuleHolder::new(module)),
            yield_points,
            call_sites: Vec::new(),
            closure_protos: Vec::new(),
            templates: Vec::new(),
        }
    }

    /// Create a JitCode with call sites but no real compiled code.
    /// For testing `elle_jit_yield_through_call` without Cranelift compilation.
    #[allow(dead_code)]
    pub(crate) fn test_with_call_sites(call_sites: Vec<super::dispatch::CallSiteMeta>) -> Self {
        let mut code = Self::test_with_yield_points(Vec::new());
        code.call_sites = call_sites;
        code
    }
}

impl std::fmt::Debug for JitCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JitCode")
            .field("fn_ptr", &self.fn_ptr)
            .finish()
    }
}
