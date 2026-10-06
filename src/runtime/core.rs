// audited: 2026-10-21
//! `RuntimeCore`: the per-instance owner bundle, and the construction that points its VM at the rest.
//!
//! docs/impl/region/rules.md
//! docs/impl/image/boot.md
//!
//! One instance is one heap, one VM, one symbol table and one compile context,
//! each boxed so the raw pointers the VM holds stay valid across the move out
//! of the constructor. boot.rs holds the boot that fills it.

use std::sync::Arc;

use super::boot::{hydrate_boot_image, BootCaches, BootFrom};
use crate::image::boot::OpenImage;
use crate::pipeline::CompileCtx;
use crate::register_primitives;
use crate::symbol::SymbolTable;
use crate::vm::VM;

/// The per-instance owner bundle: the `FiberHeap`, the `VM`, the `SymbolTable`,
/// the per-instance compile-time state (`CompileCtx`), and the resident
/// primitive metadata. Two embedded Elle instances in one process each own one
/// privately, so neither sees the other's regions, stdlib exports, or REPL
/// definitions. Both [`super::Runtime`] (the `elle foo.lisp`/REPL/embedding path) and
/// the `sys/spawn` worker construct one.
///
/// The members are boxed where an address must stay stable across the move out
/// of a constructor. The `VM` holds a raw pointer to the `SymbolTable`, the
/// `CompileCtx` and the `FiberHeap` (`set_symbols` / `set_compile_ctx` /
/// `heap_ptr`). The runtime `eval` instruction, value name-resolution, and every
/// allocation/RC operation reach them through those pointers.
///
/// The heap is a sibling of the `VM`, not a field inside it. The `VM` reaches it
/// only through `heap_ptr`, so a `&mut VM` reborrow and a `&mut FiberHeap`
/// reborrow never alias one allocation. That is the soundness contract
/// `ctx.vm()` + `ctx.heap_mut()` already rely on. Declared after the
/// `VM`/`CompileCtx` so it drops last — after teardown has run and after the
/// pointer-holders drop.
pub struct RuntimeCore {
    vm: Box<VM>,
    symbols: Box<SymbolTable>,
    compile: Box<CompileCtx>,
    /// This instance's region store. The program VM and the `CompileCtx`'s
    /// macro-expansion VM both point their `heap_ptr` here, so an instance is one
    /// heap. Two coexisting instances own two distinct heaps.
    heap: Box<crate::value::fiberheap::FiberHeap>,
    /// Kept resident for the core's life (primitive signal/arity metadata).
    _meta: crate::primitives::def::PrimitiveMeta,
}

impl RuntimeCore {
    /// Build a core: a primitives-registered VM + symbol table and a fresh
    /// `CompileCtx` (core.lisp + prelude). No stdlib: the caller, which knows
    /// its lifecycle, loads it. Uses the process-default Unicode generation.
    pub fn bare() -> Self {
        Self::bare_with_unicode(crate::config::get().unicode_generation())
    }

    /// Build a core whose VMs (program and macro) segment strings under the
    /// given Unicode generation for their whole lives.
    pub fn bare_with_unicode(gen: crate::segment::Generation) -> Self {
        Self::for_boot(gen, &[]).0
    }

    /// Boot a core with the stdlib under `caches`, as `Runtime::with_caches`
    /// does, for a holder that never tears it down: a core has no teardown
    /// sweep to run on drop.
    pub fn with_caches(caches: BootCaches) -> Self {
        let gen = crate::config::get().unicode_generation();
        Self::boot(gen, true, BootFrom::Caches(caches)).core
    }

    /// Build a core out of the first of `images` this binary accepts, or out
    /// of the three sources when it accepts none (docs/impl/image/boot.md).
    ///
    /// Answers the core, the hydrated image and the open image it came from,
    /// because the caller's stdlib step installs out of the same image the
    /// core's compile context did, and its workers hydrate the same descriptor.
    pub(super) fn for_boot(
        gen: crate::segment::Generation,
        images: &[Arc<OpenImage>],
    ) -> (Self, Option<(crate::image::boot::Boot, Arc<OpenImage>)>) {
        // This instance's heap, owned here and shared by the program VM and the
        // macro-expansion VM. Built first: both VMs point their `heap_ptr` at it,
        // so the instance is one region store and core.lisp/stdlib closures
        // (created on a VM) and runtime values all coexist in it. The `Box` has a
        // stable address the raw `heap_ptr`s alias.
        let mut heap = Box::new(crate::value::fiberheap::FiberHeap::new());
        let heap_ptr: *mut crate::value::fiberheap::FiberHeap = &mut *heap;
        let mut vm = Box::new(VM::new_with_heap(heap_ptr));
        vm.set_unicode_generation(gen);
        let mut symbols = Box::new(SymbolTable::new());
        let t = std::time::Instant::now();
        let meta = register_primitives(&mut vm, &mut symbols);
        crate::phase!(crate::trace::boot(), "boot", t, "primitives");
        // Point the VM at this instance's symbol table (stable boxed address),
        // so the runtime `eval` instruction, the meta/read/debug primitives, and
        // value name-resolution resolve in THIS instance's own table. Mirrors
        // `set_compile_ctx` below.
        vm.set_symbols(&mut *symbols as *mut SymbolTable);
        let hydrated = hydrate_boot_image(&mut heap, &mut symbols, images);
        // The macro VM shares this instance's heap (see `bare`'s heap comment).
        let mut compile = Box::new(match &hydrated {
            Some((boot, _)) => CompileCtx::from_boot_image(heap_ptr, boot, &mut symbols),
            None => CompileCtx::new_with_heap(heap_ptr),
        });
        compile.set_unicode_generation(gen);
        // The runtime `eval` instruction resolves macros/exports through this
        // instance's compile context; point the VM at it (stable boxed address).
        vm.set_compile_ctx(&mut *compile as *mut CompileCtx);
        (
            RuntimeCore {
                vm,
                symbols,
                compile,
                heap,
                _meta: meta,
            },
            hydrated,
        )
    }

    /// The three disjoint borrows a boot dump takes: the heap for the graph,
    /// the table for the spellings that travel beside it, and the compile
    /// context for the root set. Separate boxed fields, so no two alias.
    pub(crate) fn dump_parts(
        &mut self,
    ) -> (
        &mut crate::value::fiberheap::FiberHeap,
        &mut SymbolTable,
        &CompileCtx,
    ) {
        (&mut self.heap, &mut self.symbols, &self.compile)
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
    /// on one thread each get their own.
    pub fn heap(&mut self) -> &mut crate::value::fiberheap::FiberHeap {
        &mut self.heap
    }

    /// The three disjoint borrows the pipeline needs at once: the VM (execution),
    /// the symbol table (interning/resolution, shared with execution), and the
    /// compile context (macro expansion, meta, the REPL layer).
    pub fn parts(&mut self) -> (&mut VM, &mut SymbolTable, &mut CompileCtx) {
        (&mut self.vm, &mut self.symbols, &mut self.compile)
    }

    /// The same three, shared: for a reader that holds them beside a borrow of
    /// its own state, as the language server's handlers do.
    pub(crate) fn parts_shared(&self) -> (&VM, &SymbolTable, &CompileCtx) {
        (&self.vm, &self.symbols, &self.compile)
    }

    /// The heap and the symbol table as disjoint borrows — the pair the image
    /// dumper and hydrator take: the heap for the value graph, the table for
    /// the spellings that travel beside it (docs/impl/image.md).
    pub fn heap_and_symbols(
        &mut self,
    ) -> (&mut crate::value::fiberheap::FiberHeap, &mut SymbolTable) {
        (&mut self.heap, &mut self.symbols)
    }

    /// The compile context and this instance's heap as disjoint borrows — the
    /// pair [`CompileCtx::register_host_binding`] and
    /// [`CompileCtx::register_repl_binding`] need (each roots the binding's
    /// region through the heap). They are separate boxed fields, so the two
    /// `&mut` never alias; an embedder registering a host primitive reaches both
    /// without the `vm.heap_ptr` raw-pointer dance the in-crate REPL uses.
    pub fn compile_and_heap(
        &mut self,
    ) -> (&mut CompileCtx, &mut crate::value::fiberheap::FiberHeap) {
        (&mut self.compile, &mut self.heap)
    }
}
