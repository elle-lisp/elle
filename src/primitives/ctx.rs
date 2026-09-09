// audited: 2026-09-09
//! Allocation capabilities for native code (docs/impl/region/ctx.md).
//! `Alloc` carries a call's region and heap; `NativeCtx` wraps it with the
//! driving VM. A native cannot allocate without being handed one, so every
//! value names the region it is born in.

use crate::hir::region::RuntimeRegion;
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::HeapObject;
use crate::value::region_slice::RegionSlice;
use crate::value::Value;
use crate::vm::VM;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

/// The pure allocation capability: the call's freshly minted result region plus
/// heap access, and the `ctx.*` constructor surface — but **no VM**, so it cannot
/// re-enter the interpreter. Built at allocation-only sites with no VM in scope
/// (the reader, the io completion builders, `send` reconstruction, plugin ctors,
/// FFI arg marshalling, test scaffolding). A native-call capability
/// [`NativeCtx`] wraps it with a VM and `Deref`s to it (docs/impl/region/ctx.md
/// "The capability split"). It cannot be stored (borrows the call) and cannot be
/// forged (private fields).
pub struct Alloc<'h> {
    /// The call's own result region. The region every `ctx.*` allocation is
    /// born in (Rule 3).
    region: RuntimeRegion,
    /// The call's heap, as a raw pointer guarded by a phantom borrow. Raw (not
    /// `&'h mut`) so every ergonomic constructor can be `&self` and still route
    /// allocation through the ctx's OWN heap — which keeps nested calls like
    /// `ctx.pair(ctx.string(a), b)` borrow-checking (each transient `&mut
    /// FiberHeap` reborrow drops before the next allocation reborrows), while
    /// the ctx owns its heap capability outright. The `PhantomData` keeps the
    /// lifetime contract: the ctx cannot outlive the `&'h mut FiberHeap` it
    /// was built from.
    heap: *mut FiberHeap,
    _heap: PhantomData<&'h mut FiberHeap>,
}

impl<'h> Alloc<'h> {
    /// Mint the call's fresh result region from `heap` and own it — the
    /// boundary / WASM-host / signal-payload / test constructor. The result
    /// escapes to the caller (returned / marshaled across an ABI) and is freed
    /// value-based by the consumer's `DecrefValueRegion`, so the ctx holds no
    /// `Drop`. Crate-private: only the dispatch sites enumerated in
    /// docs/impl/region/ctx.md may mint a ctx. (Trait-method dispatch is NOT one:
    /// it runs the resolved method against the outer call's existing `ctx`, so a
    /// fresh method result lands in that call's `alloc_region` — see
    /// `traitregistry::call_method_fn`.)
    pub(crate) fn new(heap: &'h mut FiberHeap) -> Self {
        let region = heap.new_runtime_region();
        Self::with_region(region, heap)
    }

    /// Build a ctx over an EXPLICIT, caller-resolved region — the bytecode
    /// dispatch constructor (`dispatch_native_call`, `run_alloc_intrinsic`).
    /// The region is the solver's per-call result slot (resolved by
    /// `new_runtime_region_for_call_slot`, the merge hook), NOT a fresh mint, so
    /// the pass-through retain and the declaration oracle key off the same
    /// region the call always used.
    pub(crate) fn with_region(region: RuntimeRegion, heap: &'h mut FiberHeap) -> Self {
        Alloc {
            region,
            heap: heap as *mut FiberHeap,
            _heap: PhantomData,
        }
    }

    /// A ctx for a native-call *boundary* with no compiler-assigned result slot
    /// — the JIT/WASM host trampolines and the signal-payload builders
    /// (docs/impl/region/ctx.md). It mints its **own** fresh result region,
    /// exactly like [`new`](Self::new); the native's result escapes to the
    /// caller and is freed value-based by the consumer's `DecrefValueRegion`.
    pub(crate) fn boundary(heap: &'h mut FiberHeap) -> Self {
        Self::new(heap)
    }

    /// A ctx whose values a Rust holder keeps for the life of the instance —
    /// the stdlib cache's reload (docs/impl/region/ctx.md). It mints a fresh
    /// region like [`new`](Self::new) and records it as a process root, so the
    /// teardown sweep decrefs it and the cascade takes the values with it.
    /// Rooting at the mint is why no constructor hands the region back.
    pub(crate) fn process_root(heap: &'h mut FiberHeap) -> Self {
        let region = heap.new_runtime_region();
        crate::value::arena::register_process_root_region(heap, region);
        Self::with_region(region, heap)
    }

    /// Test-only view of the ctx's own region. Exists ONLY under `cfg(test)` so
    /// the spec-pin tests can assert "born in the ctx's region". It is invisible
    /// to production code, which has no region getter at all
    /// (docs/impl/region/ctx.md: the ctx owns its region and exposes no way to
    /// read it).
    #[cfg(test)]
    pub(crate) fn test_region(&self) -> RuntimeRegion {
        self.region
    }

    /// The ctx's own heap. Private; every allocator reborrows it for exactly
    /// one allocation statement (`&self` — see the `heap` field doc).
    ///
    /// `&self -> &mut FiberHeap` is intentional: the `ctx.*` ergonomic
    /// constructors are `&self` so nested calls like `ctx.pair(ctx.string(a), b)`
    /// borrow-check (each transient reborrow drops before the next). The heap is
    /// a raw pointer guarded by `PhantomData`, reborrowed per allocation; the
    /// SAFETY argument below is the contract `clippy::mut_from_ref` cannot see.
    #[allow(clippy::mut_from_ref)]
    #[inline]
    fn heap(&self) -> &mut FiberHeap {
        // SAFETY: the pointer is a live `&'h mut FiberHeap` reborrowed at
        // construction; `PhantomData<&'h mut FiberHeap>` ties the ctx's lifetime
        // to it. No other reference to this heap is live across a `ctx.*`
        // allocation — the primitive body runs synchronously and the VM does not
        // touch the heap during the call.
        unsafe { &mut *self.heap }
    }

    /// The ctx's own heap, for the RC funnels a native body calls directly
    /// (`arena::region_of`/`decref_region`/`push_with_incref`/…). These operate on
    /// *arbitrary* values' regions, not the call's result region, so they need the
    /// heap rather than the region-bearing `ctx.*` allocators. Same per-allocation
    /// reborrow contract as the private `heap()` (see its doc); `pub(crate)` so
    /// only in-crate native bodies reach it.
    #[allow(clippy::mut_from_ref)]
    #[inline]
    pub(crate) fn heap_mut(&self) -> &mut FiberHeap {
        self.heap()
    }

    /// Allocate a heap object into the ctx's region (Rule 3: born in the
    /// right region) on the ctx's own heap.
    pub fn alloc(&self, obj: HeapObject) -> Value {
        let region = self.region;
        self.heap().alloc_in_region(obj, region)
    }

    /// Allocate a `RegionSlice` payload into the ctx's region — it shares
    /// the region of the object that will embed it (region/model.md).
    pub fn alloc_slice<T: Copy + 'static>(&self, items: &[T]) -> RegionSlice<T> {
        let region = self.region;
        self.heap().alloc_region_slice_in_region(items, region)
    }

    /// The call's region as a syntax arena, for a native body that BUILDS a
    /// syntax tree rather than copying one it was handed. The tree lands in
    /// the same region the wrapping `ctx.syntax` object will, which is the
    /// invariant a syntax `Value` keeps (docs/impl/syntax.md § "A syntax
    /// `Value` owns its tree").
    pub fn syntax_arena(&self) -> crate::syntax::SyntaxArena {
        let region = self.region;
        crate::syntax::SyntaxArena::new(self.heap(), region)
    }

    /// Build the stored form of a struct key in the ctx's region — the
    /// forwarder a native body uses before putting a key into a struct it
    /// builds here (docs/impl/values.md § "Struct keys").
    pub fn intern_key(&self, key: &crate::value::heap::TableKey) -> crate::value::heap::TableKey {
        let region = self.region;
        key.intern_into(self.heap(), region)
    }
}

/// The native-call capability: an [`Alloc`] plus the **non-null** driving VM
/// (docs/impl/region/ctx.md "The capability split"). `Deref`s to `Alloc`, so
/// every `ctx.string(..)`/`ctx.alloc(..)`/`ctx.error(..)` works unchanged, and
/// adds [`vm`](Self::vm) for state access and synchronous interpreter re-entry.
/// Built where a VM drives the call: bytecode dispatch and the JIT/WASM hosts.
/// (Trait-method dispatch reuses the outer call's `NativeCtx` rather than
/// building one.) The `PrimFn` signature carries a `&mut NativeCtx`.
pub struct NativeCtx<'h> {
    alloc: Alloc<'h>,
    /// The driving VM, as a raw pointer guarded by the phantom borrow. Non-null
    /// by construction — a native runs only while a VM drives it, so `vm()` is
    /// total. Reborrowed per call under the contract that the VM does not touch
    /// itself during the synchronous primitive call (the same contract the heap
    /// pointer relies on).
    vm: *mut VM,
    _vm: PhantomData<&'h mut VM>,
}

impl<'h> Deref for NativeCtx<'h> {
    type Target = Alloc<'h>;
    #[inline]
    fn deref(&self) -> &Alloc<'h> {
        &self.alloc
    }
}

impl<'h> DerefMut for NativeCtx<'h> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Alloc<'h> {
        &mut self.alloc
    }
}

impl<'h> NativeCtx<'h> {
    /// Build over an EXPLICIT, caller-resolved region plus the driving VM — the
    /// bytecode dispatch constructor (`dispatch_native_call`). The region is the
    /// solver's per-call result slot; `vm` is the dispatching VM (non-null).
    pub(crate) fn with_region_vm(
        region: RuntimeRegion,
        heap: &'h mut FiberHeap,
        vm: *mut VM,
    ) -> Self {
        debug_assert!(!vm.is_null(), "NativeCtx requires a non-null VM");
        NativeCtx {
            alloc: Alloc::with_region(region, heap),
            vm,
            _vm: PhantomData,
        }
    }

    /// A native-call *boundary* with no compiler-assigned result slot — the
    /// JIT/WASM host trampolines and intrinsic re-entry. Mints a fresh result
    /// region from the VM's own heap and carries the VM. The native's result
    /// escapes to the caller and is freed value-based by the consumer's
    /// `DecrefValueRegion`.
    pub(crate) fn boundary_vm(vm: &'h mut VM) -> Self {
        let vm_ptr: *mut VM = vm as *mut VM;
        let heap: &'h mut FiberHeap = unsafe { &mut *vm.heap_ptr };
        let region = heap.new_runtime_region();
        NativeCtx {
            alloc: Alloc::with_region(region, heap),
            vm: vm_ptr,
            _vm: PhantomData,
        }
    }

    /// The driving VM. Total: a native always runs under a VM. The returned
    /// `&mut VM` is a per-call reborrow of the raw pointer; the contract is that
    /// the VM does not touch itself during the synchronous call — the same one
    /// the ctx's heap pointer relies on.
    #[allow(clippy::mut_from_ref)]
    #[inline]
    pub fn vm(&self) -> &mut VM {
        // SAFETY: `vm` is a live `&'h mut VM` reborrowed at construction and
        // non-null by construction; `PhantomData<&'h mut VM>` ties the ctx's
        // lifetime to it. The VM and the heap are disjoint allocations, so the
        // `&mut VM` here and a `&mut FiberHeap` from `self.alloc` never overlap.
        unsafe { &mut *self.vm }
    }

    /// This instance's display memo, for a message that has to show a name.
    /// `None` for an instance with no symbol table.
    ///
    /// The borrow is a reborrow of the VM's raw pointer, sound by the same
    /// contract as `vm()`. Hand it to a routine that renders a name and takes
    /// no other borrow of the ctx — the returned reference does not outlive
    /// that call, so the ctx stays free for the `ctx.error(…)` that follows.
    pub fn symbols(&self) -> Option<&crate::symbol::SymbolTable> {
        unsafe { self.vm().symbols_ptr.as_ref() }
    }

    /// The spelling of a keyword value, through this instance's memo and the
    /// static vocabulary. `None` if `v` is not a keyword or the spelling was
    /// never learned (docs/impl/symbol.md § "The display memo").
    pub fn keyword_spelling(&self, v: crate::value::Value) -> Option<String> {
        let hash = v.keyword_hash()?;
        crate::value::keyword::resolve_keyword_name(self.symbols(), hash).map(str::to_string)
    }

    /// Learn `name` into this instance's memo and build the keyword value —
    /// the mint path for a spelling that exists only at run time (a string
    /// conversion, a parsed JSON key).
    pub fn keyword(&mut self, name: &str) -> crate::value::Value {
        if let Some(symbols) = unsafe { self.vm().symbols_ptr.as_mut() } {
            symbols.keyword(name);
        }
        crate::value::Value::keyword(name)
    }

    /// The VM's Unicode segmentation generation, for grapheme operations.
    #[inline]
    pub fn unicode_generation(&self) -> crate::segment::Generation {
        self.vm().unicode_generation()
    }

    /// Where this instance caches its compiled stdlib — what a worker spawned
    /// from here inherits.
    pub fn stdlib_cache(&self) -> crate::compiler::stdlib_cache::StdlibCache {
        self.vm().stdlib_cache().clone()
    }
}

mod build;
mod scaffold;

// The test seams keep their `primitives::ctx::` path: the external `tests/`
// crates reach them by name, and which file they sit in is this module's
// business rather than theirs.
pub use scaffold::{with_test_ctx, with_test_ctx_keep_region, with_test_ctx_symbols, TestHeap};

#[cfg(test)]
mod tests;
