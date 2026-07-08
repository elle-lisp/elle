//! The process runtime: one lifecycle for compile/evaluate, shared by every
//! entry path (`elle foo.lisp`, the REPL, and the embedding API).
//!
//! A [`Runtime`] owns the VM, symbol table, and per-instance compile state
//! (a [`RuntimeCore`], which points the VM at the symbol table and compile
//! context), registers primitives and
//! (optionally) loads the stdlib, and — on `Drop` or an explicit
//! [`Runtime::teardown`] — runs the **process teardown sweep** specified in
//! docs/impl/region/rules.md § "Teardown — every region frees":
//!
//! 1. **RC-driven, never iterate-and-free.** It releases the registered process
//!    roots (each decref'd once) and lets the ordinary region RC cascade reclaim
//!    everything reachable. It never walks the region table freeing entries —
//!    that would mask the very leaks this contract exists to surface.
//! 2. **Observable.** It returns a [`TeardownReport`] naming the live region
//!    residue (the open leaks); the end-state target is zero regions remaining.
//!
//! Because all three paths run this one routine, their teardown behaviour cannot
//! drift. The naive user model holds: `elle foo.lisp` is
//! `(eval (wrap-in-letrec (read-all (slurp "foo.lisp"))))`, and afterward the
//! world returns to its pre-`main` state — only the native-fn primitives persist
//! (immediates, occupying no region).

use crate::pipeline::CompileCtx;
use crate::symbol::SymbolTable;
use crate::vm::VM;
use crate::{init_stdlib, register_primitives};

/// The observable result of a teardown sweep (docs/impl/region/rules.md §
/// "Teardown — every region frees", property 2). The standing target is
/// `live_regions == 0`; a non-zero value is the current set of open leaks — the
/// remaining work, not a tuning knob.
#[derive(Debug, Clone)]
pub struct TeardownReport {
    /// Number of live regions after the sweep. Zero is the end-state; anything
    /// else is leaked. (Every region is mortal, so this is simply the count of
    /// regions whose RC never reached zero.)
    pub live_regions: usize,
    /// `(id, rc, object_count)` for each surviving region — names *what* leaked,
    /// for the residue diagnostic.
    pub regions: Vec<(u32, u32, usize)>,
    /// How many registered process roots the sweep released by RC.
    pub roots_released: usize,
}

/// The per-instance owner bundle: the `FiberHeap`, the `VM`, the `SymbolTable`,
/// the per-instance compile-time state (`CompileCtx`), and the resident
/// primitive metadata. Two embedded Elle instances in one process each own one
/// privately, so neither sees the other's regions, stdlib exports, or REPL
/// definitions. Both [`Runtime`] (the `elle foo.lisp`/REPL/embedding path) and
/// the `os/spawn` worker construct one.
///
/// The members are boxed where an address must stay stable across the move out
/// of a constructor: the `SymbolTable`, `CompileCtx`, and `FiberHeap` because the
/// `VM` holds a raw pointer to each (`set_symbols` / `set_compile_ctx` /
/// `heap_ptr`), through which the runtime `eval` instruction, value
/// name-resolution, and every allocation/RC operation reach them.
///
/// The heap is a sibling of the `VM`, not a field inside it: the `VM` reaches it
/// only through `heap_ptr`, so a `&mut VM` reborrow and a `&mut FiberHeap`
/// reborrow never alias one allocation (the soundness contract `ctx.vm()` +
/// `ctx.heap_mut()` already rely on). Declared after the `VM`/`CompileCtx` so it
/// drops last — after teardown has run and after the pointer-holders drop.
pub struct RuntimeCore {
    vm: Box<VM>,
    symbols: Box<SymbolTable>,
    compile: Box<CompileCtx>,
    /// This instance's region store. The program VM and the `CompileCtx`'s
    /// macro-expansion VM both point their `heap_ptr` here, so an instance is one
    /// heap. Two coexisting instances own two distinct heaps (tls.md).
    heap: Box<crate::value::fiberheap::FiberHeap>,
    /// Kept resident for the core's life (primitive signal/arity metadata).
    _meta: crate::primitives::def::PrimitiveMeta,
}

impl RuntimeCore {
    /// Build a core: a primitives-registered VM + symbol table and a fresh
    /// `CompileCtx` (core.lisp + prelude). No stdlib, no thread-local contexts —
    /// the caller (which knows its lifecycle) drives those.
    pub fn bare() -> Self {
        // This instance's heap, owned here and shared by the program VM and the
        // macro-expansion VM. Built first: both VMs point their `heap_ptr` at it,
        // so the instance is one region store and core.lisp/stdlib closures
        // (created on a VM) and runtime values all coexist in it. The `Box` has a
        // stable address the raw `heap_ptr`s alias.
        let mut heap = Box::new(crate::value::fiberheap::FiberHeap::new());
        let heap_ptr: *mut crate::value::fiberheap::FiberHeap = &mut *heap;
        let mut vm = Box::new(VM::new_with_heap(heap_ptr));
        let mut symbols = Box::new(SymbolTable::new());
        let meta = register_primitives(&mut vm, &mut symbols);
        // Point the VM at this instance's symbol table (stable boxed address),
        // so the runtime `eval` instruction, the meta/read/debug primitives, and
        // value name-resolution resolve in THIS instance's own table. Mirrors
        // `set_compile_ctx` below.
        vm.set_symbols(&mut *symbols as *mut SymbolTable);
        // The macro VM shares this instance's heap (see `bare`'s heap comment).
        let mut compile = Box::new(CompileCtx::new_with_heap(heap_ptr));
        // The runtime `eval` instruction resolves macros/exports through this
        // instance's compile context; point the VM at it (stable boxed address).
        vm.set_compile_ctx(&mut *compile as *mut CompileCtx);
        RuntimeCore {
            vm,
            symbols,
            compile,
            heap,
            _meta: meta,
        }
    }

    /// Compile and execute stdlib.lisp into this core's `CompileCtx`. The caller
    /// must have installed the symbol-table context first (stdlib macros gensym).
    pub fn load_stdlib(&mut self) {
        let (vm, symbols, compile) = self.parts();
        init_stdlib(vm, symbols, compile);
    }

    /// Mutable access to the VM.
    pub fn vm(&mut self) -> &mut VM {
        &mut self.vm
    }

    /// Mutable access to the symbol table.
    pub fn symbols(&mut self) -> &mut SymbolTable {
        &mut self.symbols
    }

    /// Mutable access to the compile context.
    pub fn compile(&mut self) -> &mut CompileCtx {
        &mut self.compile
    }

    /// Mutable access to this instance's fiber heap — the region/RC store every
    /// allocation and reference-count operation reads through. This is the
    /// core-owned `Box<FiberHeap>` that the VM's `heap_ptr` aliases; reaching it
    /// directly keeps the borrow disjoint from a `&mut VM`. Two embedded instances
    /// on one thread each get their own (tls.md § Acceptance criterion); a shared
    /// per-thread heap is the coexistence defect this axis removes.
    pub fn heap(&mut self) -> &mut crate::value::fiberheap::FiberHeap {
        &mut self.heap
    }

    /// The three disjoint borrows the pipeline needs at once: the VM (execution),
    /// the symbol table (interning/resolution, shared with execution), and the
    /// compile context (macro expansion, meta, projections).
    pub fn parts(&mut self) -> (&mut VM, &mut SymbolTable, &mut CompileCtx) {
        (&mut self.vm, &mut self.symbols, &mut self.compile)
    }

    /// The compile context and this instance's heap as disjoint borrows — the
    /// pair [`CompileCtx::register_repl_binding`] needs (it roots the binding's
    /// region through the heap). They are separate boxed fields, so the two
    /// `&mut` never alias; an embedder registering a host primitive reaches both
    /// without the `vm.heap_ptr` raw-pointer dance the in-crate REPL uses.
    pub fn compile_and_heap(
        &mut self,
    ) -> (&mut CompileCtx, &mut crate::value::fiberheap::FiberHeap) {
        (&mut self.compile, &mut self.heap)
    }
}

/// The process runtime. Construct once per entry path; drop (or call
/// [`teardown`](Runtime::teardown)) to run the principled sweep.
pub struct Runtime {
    core: RuntimeCore,
    torn_down: bool,
}

impl Runtime {
    /// Build a runtime with primitives registered and the stdlib loaded — the
    /// configuration `elle foo.lisp`, the REPL, and ordinary embedding use.
    pub fn new() -> Self {
        Self::build(true)
    }

    /// Build a runtime with primitives only, no stdlib — for `--no-stdlib` and
    /// for teardown tests that need a clean region baseline.
    pub fn without_stdlib() -> Self {
        Self::build(false)
    }

    fn build(load_stdlib: bool) -> Self {
        let mut core = RuntimeCore::bare();

        // `RuntimeCore::bare` already pointed the VM at this instance's symbol
        // table, so stdlib-load gensym (and all runtime name resolution) resolve
        // through `ctx.vm().symbols()` — this instance's own table.
        if load_stdlib {
            core.load_stdlib();
        }

        Runtime {
            core,
            torn_down: false,
        }
    }

    /// Mutable access to the VM.
    pub fn vm(&mut self) -> &mut VM {
        self.core.vm()
    }

    /// Mutable access to the symbol table.
    pub fn symbols(&mut self) -> &mut SymbolTable {
        self.core.symbols()
    }

    /// Mutable access to the compile context.
    pub fn compile(&mut self) -> &mut CompileCtx {
        self.core.compile()
    }

    /// Mutable access to this instance's fiber heap (see
    /// [`RuntimeCore::heap`]).
    pub fn heap(&mut self) -> &mut crate::value::fiberheap::FiberHeap {
        self.core.heap()
    }

    /// The disjoint VM / symbol-table / compile-context borrows — most
    /// run/compile entry points need them simultaneously, which separate `&mut`
    /// method calls cannot provide.
    pub fn parts(&mut self) -> (&mut VM, &mut SymbolTable, &mut CompileCtx) {
        self.core.parts()
    }

    /// The compile context and this instance's heap as disjoint borrows — the
    /// pair an embedder hands to [`CompileCtx::register_repl_binding`] (see
    /// [`RuntimeCore::compile_and_heap`]).
    pub fn compile_and_heap(
        &mut self,
    ) -> (&mut CompileCtx, &mut crate::value::fiberheap::FiberHeap) {
        self.core.compile_and_heap()
    }

    /// Run the process teardown sweep and return its observable report. RC-driven
    /// (roots released → cascade), never iterate-and-free. Idempotent: a second
    /// call releases nothing further (the registry was drained) and re-reports
    /// the residue.
    pub fn teardown(&mut self) -> TeardownReport {
        self.torn_down = true;

        // (1) Release the per-instance compile-time state's resident references:
        //     the pre-compiled macro transformers hold region references their
        //     `Copy` `Value`s would never decref. The `CompileCtx` itself (macro
        //     VM, expander, meta) drops with this core; the region pages its
        //     `Value`s alias are reclaimed by the RC sweep below.
        // Reach the heap through the VM's `heap_ptr` (a `Copy` raw pointer, read
        // out so it holds no borrow of `self.core`) so the `&mut CompileCtx`
        // release borrow and the `&mut FiberHeap` it needs are disjoint.
        let heap_ptr = self.core.vm().heap_ptr;
        self.core.compile.release(unsafe { &mut *heap_ptr });
        // The default trait tables are `alloc_root`'d into this instance's root
        // region the RC sweep below releases; clear the heap's table so a later
        // read sees `NIL` instead of `Value`s pointing into the freed region.
        crate::primitives::traitregistry::reset_default_traits(unsafe { &mut *heap_ptr });

        // (2) The one heap action: release this instance's registered process
        //     roots by RC and let the cascade reclaim everything reachable.
        let roots_released =
            crate::value::arena::teardown_process_root_regions(unsafe { &mut *heap_ptr });

        // (3) Observe the result. Every region is mortal, so every surviving
        //     region is leaked residue.
        let regions: Vec<(u32, u32, usize)> = unsafe { &*heap_ptr }.region_info_vec();

        TeardownReport {
            live_regions: regions.len(),
            regions,
            roots_released,
        }
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if !self.torn_down {
            let _ = self.teardown();
        }
        // Nothing to restore: the VM's symbol-table pointer drops with the core.
        // The next `Runtime` on this thread builds a fresh core.
    }
}

#[cfg(test)]
mod tests;
