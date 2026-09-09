// audited: 2026-09-09
//! Test seams that hand out a capability over a throwaway VM, so test code
//! calls a primitive and builds values the way production natives do.
//! docs/impl/region/ctx.md

use super::NativeCtx;
use crate::hir::region::RuntimeRegion;
use crate::vm::VM;

/// Test support: run `f` with a NativeCtx over a fresh region on the VM's heap,
/// releasing the region afterward. The seam test code uses to call a
/// primitive directly (`with_test_ctx(|ctx| prim_x(ctx, &args))`).
///
/// `#[doc(hidden)] pub` rather than `#[cfg(test)]`: the external integration
/// test crates (`tests/`) compile against the non-test library and so cannot
/// see `#[cfg(test)]` items, yet they call native primitives directly
/// (`tests/property/ffi.rs`, …) and need this seam.
#[doc(hidden)]
pub fn with_test_ctx<R>(f: impl FnOnce(&mut NativeCtx) -> R) -> R {
    // A real VM so `ctx.vm()` is valid for primitives that read VM state or
    // re-enter the interpreter; `f` allocates through the ctx over a fresh
    // region on the VM's heap (docs/impl/region/ctx.md).
    let mut vm = crate::vm::VM::new();
    let vm_ptr: *mut VM = &mut vm as *mut VM;
    let heap_ptr = vm.heap_ptr;
    let region = unsafe { (*heap_ptr).new_runtime_region() };
    let out = {
        let mut ctx = NativeCtx::with_region_vm(region, unsafe { &mut *heap_ptr }, vm_ptr);
        f(&mut ctx)
    };
    unsafe { (*heap_ptr).decref_region_if_present(region) };
    out
}

/// Like [`with_test_ctx`], but does NOT release the region afterward, so a
/// heap-backed return value (an error struct, a `disbit` array of strings, …)
/// stays valid for the caller to inspect.
///
/// `with_test_ctx` releases the region on return, freeing its objects; the next
/// allocation could reuse those pages, so a returned heap `Value` read afterward
/// would be a use-after-free. Use this seam when the caller must read the
/// returned `Value`'s heap contents after the call: the region is kept, and the
/// VM's heap is leaked (`VM::new`), so its objects outlive the dropped VM.
#[doc(hidden)]
pub fn with_test_ctx_keep_region<R>(f: impl FnOnce(&mut NativeCtx) -> R) -> R {
    let mut vm = crate::vm::VM::new();
    let vm_ptr: *mut VM = &mut vm as *mut VM;
    let heap_ptr = vm.heap_ptr;
    let region = unsafe { (*heap_ptr).new_runtime_region() };
    let mut ctx = NativeCtx::with_region_vm(region, unsafe { &mut *heap_ptr }, vm_ptr);
    f(&mut ctx)
}

/// Like [`with_test_ctx_keep_region`], but points the ctx's VM at `symbols`, so a
/// meta primitive that interns through the driving VM (`gensym`, `datum->syntax`,
/// `syntax->datum`) resolves names in the caller's table. The test hands the table
/// to the ctx, which the primitive reads via `ctx.vm().symbols_ptr`.
///
/// `symbols` must outlive the call — the primitive reads it while `f` runs. The
/// region is kept (a heap-backed return value stays valid for the caller); see
/// [`with_test_ctx_keep_region`].
#[doc(hidden)]
pub fn with_test_ctx_symbols<R>(
    symbols: *mut crate::symbol::SymbolTable,
    f: impl FnOnce(&mut NativeCtx) -> R,
) -> R {
    let mut vm = crate::vm::VM::new();
    vm.set_symbols(symbols);
    let vm_ptr: *mut VM = &mut vm as *mut VM;
    let heap_ptr = vm.heap_ptr;
    let region = unsafe { (*heap_ptr).new_runtime_region() };
    let mut ctx = NativeCtx::with_region_vm(region, unsafe { &mut *heap_ptr }, vm_ptr);
    f(&mut ctx)
}

/// Test-only ergonomic value builder. Owns a `VM` (whose heap carries the default
/// trait tables) and one result region, and hands out a fresh [`NativeCtx`] over
/// them via [`ctx`](Self::ctx) — so test code builds values through the same
/// `ctx.*` surface production natives use.
///
/// `#[doc(hidden)] pub` (not `#[cfg(test)]`) so the external `tests/` crates,
/// which compile against the non-test library, can build heap values too. The
/// VM's heap is leaked (`VM::new`), so the values it builds stay valid for the
/// test process even after the `TestHeap` drops.
#[doc(hidden)]
pub struct TestHeap {
    vm: Box<VM>,
    region: RuntimeRegion,
}

impl Default for TestHeap {
    fn default() -> Self {
        Self::new()
    }
}

impl TestHeap {
    pub fn new() -> Self {
        let mut vm = Box::new(VM::new());
        let region = vm.heap().new_runtime_region();
        TestHeap { vm, region }
    }

    /// A fresh allocation+VM capability over this builder's heap and region. Each
    /// call reborrows the owned VM/heap through raw pointers (the same contract
    /// `NativeCtx` itself relies on), so nested `h.ctx().pair(h.ctx().string(a), b)`
    /// builds fine — every `ctx.*` allocation lands in this builder's region.
    pub fn ctx(&self) -> NativeCtx<'_> {
        let vm_ptr = &*self.vm as *const VM as *mut VM;
        let heap_ptr = self.vm.heap_ptr;
        NativeCtx::with_region_vm(self.region, unsafe { &mut *heap_ptr }, vm_ptr)
    }

    /// The builder's VM, for tests that need VM state (e.g. a symbol table).
    pub fn vm(&mut self) -> &mut VM {
        &mut self.vm
    }

    /// This builder's heap — for serialization paths (`SendBundle::from_value`)
    /// that need the heap a value it built lives on. Same per-allocation reborrow
    /// contract as [`NativeCtx::vm`]; the heap is a disjoint allocation.
    #[allow(clippy::mut_from_ref)]
    pub fn heap(&self) -> &mut crate::value::fiberheap::FiberHeap {
        unsafe { &mut *self.vm.heap_ptr }
    }
}
