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

    /// The spelling of a keyword value, through this instance's memo and the
    /// static vocabulary. `None` if `v` is not a keyword or the spelling was
    /// never learned (docs/impl/symbol.md § "The display memo").
    pub fn keyword_spelling(&self, v: crate::value::Value) -> Option<String> {
        let hash = v.keyword_hash()?;
        let symbols = unsafe { self.vm().symbols_ptr.as_ref() };
        crate::value::keyword::resolve_keyword_name(symbols, hash).map(str::to_string)
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

/// Generate the ergonomic `ctx.*` constructors (docs/impl/region/ctx.md
/// "the body-migration surface"): each forwards to the matching
/// `value::build::*` source with the ctx's own heap and region, so a native
/// body reads `ctx.string("x")` and the value is born on the ctx's heap in the
/// ctx's region. One spec line per heap type.
macro_rules! ctx_ctors {
    ( $(
        $(#[$attr:meta])*
        $name:ident ( $($arg:ident : $ty:ty),* $(,)? ) ;
    )* ) => {
        impl<'h> Alloc<'h> {
            $(
                $(#[$attr])*
                #[inline]
                // `&self`, not `&mut self`: `self.heap()` reborrows the ctx's
                // raw heap pointer for exactly this allocation, so nested
                // constructor calls (`ctx.pair(ctx.string(a), b)`) borrow-check —
                // the inner reborrow drops before the outer one is taken.
                pub fn $name(&self $(, $arg: $ty)*) -> Value {
                    crate::value::build::$name(self.heap() $(, $arg)*, self.region)
                }
            )*
        }
    };
}

ctx_ctors! {
    /// Allocate a string into the call's region.
    string (s: impl AsRef<str>);
    /// Allocate a cons cell into the call's region.
    pair (head: Value, tail: Value);
    /// Allocate an immutable array into the call's region.
    array (elements: Vec<Value>);
    /// Allocate a mutable `@array` into the call's region.
    array_mut (elements: Vec<Value>);
    /// Allocate an empty mutable `@struct` into the call's region.
    struct_mut ();
    /// Allocate a mutable `@struct` with entries into the call's region.
    struct_mut_from (
        entries: std::collections::BTreeMap<crate::value::heap::TableKey, Value>
    );
    /// Allocate an immutable struct (from an unsorted map) into the call's region.
    struct_from (
        fields: std::collections::BTreeMap<crate::value::heap::TableKey, Value>
    );
    /// Allocate an immutable struct (from pre-sorted entries) into the call's region.
    struct_from_sorted (
        entries: Vec<(crate::value::heap::TableKey, Value)>
    );
    /// Allocate a closure into the call's region.
    closure (c: crate::value::heap::Closure);
    /// Materialize a code object's header into the call's region.
    template (proto: &std::rc::Rc<crate::value::TemplateProto>);
    /// Allocate a user box (`LBox`) into the call's region.
    lbox (value: Value);
    /// Allocate a compiler capture cell into the call's region.
    capture_cell (value: Value);
    /// Allocate a mutable `@string` into the call's region.
    string_mut (bytes: Vec<u8>);
    /// Allocate immutable bytes into the call's region.
    bytes (data: Vec<u8>);
    /// Allocate mutable `@bytes` into the call's region.
    bytes_mut (data: Vec<u8>);
    /// Allocate a syntax object into the call's region.
    syntax (s: crate::syntax::Syntax);
    /// Allocate an immutable set into the call's region.
    set (items: std::collections::BTreeSet<Value>);
    /// Allocate a mutable set into the call's region.
    set_mut (items: std::collections::BTreeSet<Value>);
    /// Allocate a managed FFI pointer into the call's region (NULL ⇒ nil).
    managed_pointer (addr: usize);
}

impl<'h> Alloc<'h> {
    /// Construct an error value `{:error :kind :message msg}` born on the ctx's
    /// heap in the call's region (the ergonomic forwarder a native body uses
    /// instead of the bare `error_val`). The kind keyword is interned
    /// (immediate).
    #[inline]
    pub fn error(&self, kind: &str, msg: impl Into<String>) -> Value {
        crate::value::build::error(self.heap(), kind, msg, self.region)
    }

    /// Construct an error value with extra context fields, born in the call's
    /// region — the ctx forwarder for the bare `error_val_extra`.
    #[inline]
    pub fn error_extra(
        &self,
        kind: &str,
        msg: impl Into<String>,
        extra: &[(&str, Value)],
    ) -> Value {
        crate::value::build::error_extra(self.heap(), kind, msg, extra, self.region)
    }

    /// The runtime no-match error for `match`, born in the call's region —
    /// the ctx forwarder for the bare `match_fail_error`.
    #[inline]
    pub fn match_fail(&self, val: Value) -> Value {
        crate::value::build::match_fail(self.heap(), val, self.region)
    }

    /// Allocate an external (plugin-provided) object into the call's region.
    /// Hand-written rather than macro-generated because of the generic `T`.
    #[inline]
    pub fn external<T: std::any::Any + 'static>(&self, type_name: &'static str, data: T) -> Value {
        crate::value::build::external(self.heap(), type_name, data, self.region)
    }

    /// Build a proper list (cons chain) into the call's region — every cell on
    /// the ctx's own heap. Hand-written rather than macro-generated because of
    /// the generic `IntoIterator`.
    #[inline]
    pub fn list(&self, values: impl IntoIterator<Item = Value>) -> Value {
        crate::value::build::list(self.heap(), values, self.region)
    }

    // ── Single-object ctors with no `value::build::*` twin ──────────────
    //
    // These wrap one `HeapObject` (no `RegionSlice` payload), so they allocate
    // directly into the call's region via `alloc`. Hand-written because their
    // arg shapes / construction logic are specific (a global id counter, a
    // handle wrapper, an FFI descriptor). They replace the bare `Value::*`
    // single-object ctors at native-call sites (RegionEffect::Fresh requires the
    // result in the call's own region, which the region-free bare ctor — minting
    // its own fresh region — would violate).

    /// Allocate a dynamic `parameter` with `default` into the call's region.
    #[inline]
    pub fn parameter(&self, default: Value) -> Value {
        use crate::value::heap::HeapObject;
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT_ID: AtomicU32 = AtomicU32::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        self.alloc(HeapObject::Parameter {
            id,
            default,
            traits: Value::NIL,
        })
    }

    /// Allocate a `fiber` into the call's region.
    #[inline]
    pub fn fiber(&self, f: crate::value::fiber::Fiber) -> Value {
        use crate::value::fiber::FiberHandle;
        use crate::value::heap::HeapObject;
        self.alloc(HeapObject::Fiber {
            handle: FiberHandle::new(f),
            traits: Value::NIL,
        })
    }

    /// Allocate a `fiber` from an existing handle into the call's region.
    #[inline]
    pub fn fiber_from_handle(&self, handle: crate::value::fiber::FiberHandle) -> Value {
        use crate::value::heap::HeapObject;
        self.alloc(HeapObject::Fiber {
            handle,
            traits: Value::NIL,
        })
    }

    /// Allocate an FFI compound type descriptor into the call's region.
    #[inline]
    pub fn ffi_type(&self, desc: crate::ffi::types::TypeDesc) -> Value {
        use crate::value::heap::HeapObject;
        self.alloc(HeapObject::FFIType(desc))
    }

    /// Allocate an FFI signature into the call's region.
    #[inline]
    pub fn ffi_signature(&self, sig: crate::ffi::types::Signature) -> Value {
        use crate::value::heap::{CifCache, HeapObject};
        #[cfg(feature = "ffi")]
        let cache: CifCache = std::cell::RefCell::new(None);
        #[cfg(not(feature = "ffi"))]
        let cache: CifCache = ();
        self.alloc(HeapObject::FFISignature(sig, cache))
    }

    /// Allocate a library handle into the call's region.
    #[inline]
    pub fn lib_handle(&self, id: u32) -> Value {
        use crate::value::heap::HeapObject;
        self.alloc(HeapObject::LibHandle(id))
    }
}

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

// ── Spec pins (docs/impl/region/ctx.md) ─────────────────────────────
//
// Written from the spec BEFORE the implementation (CLAUDE.md: docs →
// tests → code). Each pins a contract line:
//  - `alloc` routes the object into exactly the ctx's region (Rule 3:
//    born in the right region — not a sibling region).
//  - `alloc_slice` payload shares the ctx's region (region/model.md
//    "RegionSlice contents share their object's region").
//  - the captured `RuntimeRegion` (threaded by `with_ctx`, or read via the
//    `cfg(test)`-only `test_region`) is where every `ctx.*` allocation lands —
//    there is no public region getter in production (the ctx owns its region).

#[cfg(test)]
mod tests;
