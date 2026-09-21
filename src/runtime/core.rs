// audited: 2026-09-21
//! `RuntimeCore`: the per-instance owner bundle, and the boot that fills it.
//!
//! docs/impl/region/rules.md
//! docs/impl/image/boot.md
//!
//! One instance is one heap, one VM, one symbol table and one compile context,
//! each boxed so the raw pointers the VM holds stay valid across the move out
//! of the constructor. The boot either hydrates an image or compiles the three
//! sources; both arms end in the same registration tails.

use crate::compiler::stdlib_cache::StdlibCache;
use crate::image::boot::BootImage;
use crate::pipeline::CompileCtx;
use crate::symbol::SymbolTable;
use crate::vm::VM;
use crate::{init_stdlib, register_primitives};

/// The two caches one instance boots through: where its compiled stdlib is
/// cached, and where its boot image is.
///
/// Both travel with the instance rather than coming from process state
/// (docs/impl/image/boot.md), so the pair is one construction parameter.
#[derive(Debug, Clone, Default)]
pub struct BootCaches {
    /// Where the compiled stdlib bytecode is cached
    /// (docs/impl/stdlib-cache.md). Consulted only when no boot image loads.
    pub stdlib: StdlibCache,
    /// Where the boot image is (docs/impl/image/boot.md). `Off` by default.
    pub image: BootImage,
}

/// Hydrate the image `image` names, or answer `None` and leave the instance to
/// compile its three sources.
///
/// A refusal is reported and swallowed: a boot image is a speedup, and every
/// reason to refuse one — a fingerprint from another build, a digest from other
/// sources, a truncated file — is a reason to compile instead. The report is
/// what keeps a cache that never hits from looking like one that always does.
fn hydrate_boot_image(
    heap: &mut crate::value::fiberheap::FiberHeap,
    symbols: &mut SymbolTable,
    image: &BootImage,
) -> Option<crate::image::boot::Boot> {
    let path = image.path()?;
    if !path.exists() {
        return None;
    }
    let t = std::time::Instant::now();
    match crate::image::boot::hydrate_path(heap, symbols, &path) {
        Ok(boot) => {
            crate::phase!(crate::trace::boot(), "boot", t, "image-hydrate");
            Some(boot)
        }
        Err(e) => {
            eprintln!("[boot-image] {}: {e}", path.display());
            None
        }
    }
}

/// The per-instance owner bundle: the `FiberHeap`, the `VM`, the `SymbolTable`,
/// the per-instance compile-time state (`CompileCtx`), and the resident
/// primitive metadata. Two embedded Elle instances in one process each own one
/// privately, so neither sees the other's regions, stdlib exports, or REPL
/// definitions. Both [`super::Runtime`] (the `elle foo.lisp`/REPL/embedding path) and
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
    /// the caller (which knows its lifecycle) drives those. Uses the
    /// process-default Unicode generation.
    pub fn bare() -> Self {
        Self::bare_with_unicode(crate::config::get().unicode_generation())
    }

    /// Build a core whose VMs (program and macro) segment strings under the
    /// given Unicode generation for their whole lives.
    pub fn bare_with_unicode(gen: crate::segment::Generation) -> Self {
        Self::for_boot(gen, &BootImage::Off).0
    }

    /// Build a core out of the boot image `image` names, or out of the three
    /// sources when it names none this binary can hydrate
    /// (docs/impl/image/boot.md).
    ///
    /// Answers the core and the hydrated image, because the caller's stdlib
    /// step installs out of the same image the core's compile context did.
    pub(crate) fn for_boot(
        gen: crate::segment::Generation,
        image: &BootImage,
    ) -> (Self, Option<crate::image::boot::Boot>) {
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
        let boot = hydrate_boot_image(&mut heap, &mut symbols, image);
        // The macro VM shares this instance's heap (see `bare`'s heap comment).
        let mut compile = Box::new(match &boot {
            Some(boot) => CompileCtx::from_boot_image(heap_ptr, boot, &mut symbols),
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
            boot,
        )
    }

    /// Install a hydrated image's stdlib exports, in place of compiling and
    /// running stdlib.lisp. The image's region is already a process root, so
    /// this roots nothing of its own (docs/impl/image/boot.md).
    pub(crate) fn install_stdlib_from_image(&mut self, boot: &crate::image::boot::Boot) {
        let exports = boot.stdlib_exports();
        let (_vm, symbols, compile) = self.parts();
        crate::primitives::module_init::install_exports(symbols, compile, exports);
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

    /// Compile and execute stdlib.lisp into this core's `CompileCtx`. The caller
    /// must have installed the symbol-table context first (stdlib macros gensym).
    pub fn load_stdlib(
        &mut self,
        cache: &crate::compiler::stdlib_cache::StdlibCache,
    ) -> crate::primitives::module_init::StdlibSource {
        // Record it on the VM so a `sys/spawn` worker, which reaches the
        // spawning instance only through `ctx.vm()`, inherits this directory
        // instead of falling back to the process-wide one.
        self.vm.set_stdlib_cache(cache.clone());
        let (vm, symbols, compile) = self.parts();
        init_stdlib(vm, symbols, compile, cache)
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

    /// The heap and the symbol table as disjoint borrows — the pair the image
    /// dumper and hydrator take: the heap for the value graph, the table for
    /// the spellings that travel beside it (docs/impl/image.md).
    pub fn heap_and_symbols(
        &mut self,
    ) -> (&mut crate::value::fiberheap::FiberHeap, &mut SymbolTable) {
        (&mut self.heap, &mut self.symbols)
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
