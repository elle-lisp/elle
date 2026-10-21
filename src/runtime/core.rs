// audited: 2026-10-20
//! `RuntimeCore`: the per-instance owner bundle, and the boot that fills it.
//!
//! docs/impl/region/rules.md
//! docs/impl/image/boot.md
//!
//! One instance is one heap, one VM, one symbol table and one compile context,
//! each boxed so the raw pointers the VM holds stay valid across the move out
//! of the constructor. The boot either hydrates an image or compiles the three
//! sources; both arms end in the same registration tails.

use std::sync::Arc;

use crate::compiler::stdlib_cache::StdlibCache;
use crate::image::boot::{BootImage, BootSource, OpenImage};
use crate::pipeline::CompileCtx;
use crate::primitives::module_init::StdlibSource;
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
    /// (docs/impl/stdlib-cache.md). The instance reads it only when it hydrates
    /// no image and owes no store; its `sys/spawn` workers read it when it
    /// holds no image.
    pub stdlib: StdlibCache,
    /// Where the boot image is (docs/impl/image/boot.md). The process-wide
    /// policy by default: the embedded blob, then the warm cache.
    pub image: BootImage,
}

impl BootCaches {
    /// The process-wide choice of both: the `--cache=` stdlib directory, and
    /// the `--boot-image=` policy.
    pub fn process() -> Self {
        BootCaches {
            stdlib: StdlibCache::Process,
            image: crate::config::get().boot_image(),
        }
    }
}

/// What a worker spawned from this instance boots from: the image this
/// instance holds, and the stdlib cache a worker compiles through when it
/// holds none (docs/impl/image/boot.md).
///
/// The image is the descriptor this instance hydrated or stored, shared, so a
/// worker maps the bytes its parent mapped whatever happened to the path since.
#[derive(Debug, Clone, Default)]
pub struct WorkerBoot {
    image: Option<Arc<OpenImage>>,
    stdlib: StdlibCache,
}

/// Where an instance boots from.
pub(crate) enum BootFrom {
    /// A fresh instance's caches: the first image the policy names that
    /// hydrates is used, and a miss stores one where the policy names a file.
    Caches(BootCaches),
    /// A worker's parent: the image it holds is hydrated, and nothing is ever
    /// stored.
    Parent(WorkerBoot),
}

/// A booted core, and where its boot state came from.
pub(crate) struct Booted {
    pub(crate) core: RuntimeCore,
    pub(crate) stdlib_source: StdlibSource,
    pub(crate) boot_source: BootSource,
}

/// Hydrate the first of `images` this binary accepts, or answer `None` and
/// leave the instance to compile its three sources.
///
/// Each refusal is reported and swallowed, because a boot image is a speedup.
/// Every reason to refuse one — a fingerprint from another build, a digest from
/// other sources, a truncated file — is a reason to try the next or compile.
/// The report is what keeps a cache that never hits from looking like one that
/// always does. The answer names the image that hydrated, which the instance
/// hands its workers.
fn hydrate_boot_image(
    heap: &mut crate::value::fiberheap::FiberHeap,
    symbols: &mut SymbolTable,
    images: &[Arc<OpenImage>],
) -> Option<(crate::image::boot::Boot, Arc<OpenImage>)> {
    for image in images {
        let t = std::time::Instant::now();
        match crate::image::boot::hydrate(heap, symbols, image.source()) {
            Ok(boot) => {
                crate::phase!(crate::trace::boot(), "boot", t, "image-hydrate");
                return Some((boot, Arc::clone(image)));
            }
            Err(e) => eprintln!("[boot-image] {}: {e}", image.origin()),
        }
    }
    None
}

/// The per-instance owner bundle: the `FiberHeap`, the `VM`, the `SymbolTable`,
/// the per-instance compile-time state (`CompileCtx`), and the resident
/// primitive metadata. Two embedded Elle instances in one process each own one
/// privately, so neither sees the other's regions, stdlib exports, or REPL
/// definitions. Both [`super::Runtime`] (the `elle foo.lisp`/REPL/embedding path) and
/// the `os/spawn` worker construct one.
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
    /// `CompileCtx` (core.lisp + prelude). No stdlib, no thread-local contexts —
    /// the caller (which knows its lifecycle) drives those. Uses the
    /// process-default Unicode generation.
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

    /// Boot a core from `from`: out of a boot image where there is one this
    /// binary hydrates, out of the three sources otherwise
    /// (docs/impl/image/boot.md). With `stdlib` false the core holds core.lisp
    /// and the prelude alone.
    ///
    /// The core records what its own workers boot from: the image it hydrated
    /// or stored, and the stdlib cache `from` names.
    pub(crate) fn boot(gen: crate::segment::Generation, stdlib: bool, from: BootFrom) -> Booted {
        // A policy's image is the whole boot state, stdlib included, so an
        // instance that wants no stdlib reads and stores none — and
        // `--no-stdlib` is how core.lisp and the prelude are debugged.
        let (policy, images, cache) = match from {
            BootFrom::Caches(caches) if stdlib => {
                let images = caches.image.images().into_iter().map(Arc::new).collect();
                (Some(caches.image), images, caches.stdlib)
            }
            BootFrom::Caches(caches) => (None, Vec::new(), caches.stdlib),
            BootFrom::Parent(parent) => (None, parent.image.into_iter().collect(), parent.stdlib),
        };
        let (mut core, hydrated) = Self::for_boot(gen, &images);
        let (stdlib_source, boot_source, kept) = match hydrated {
            Some((boot, held)) => {
                let stdlib_source = if stdlib {
                    core.install_stdlib_from_image(&boot);
                    StdlibSource::Image
                } else {
                    StdlibSource::Compiled
                };
                (stdlib_source, BootSource::Image, Some(held))
            }
            None if stdlib => {
                // A boot image is dumped from a *compiled* stdlib. A disk-cache
                // hit rebuilds the library's closures through the send codec,
                // whose capture cells record no binding, and the dump refuses
                // one of those by variant (docs/impl/image/boot.md). So an
                // instance that owes an image compiles rather than reading the
                // other cache — once per build and digest, for the start that
                // stores it.
                let compiling = match policy.as_ref().and_then(BootImage::path) {
                    Some(_) => StdlibCache::Off,
                    None => cache.clone(),
                };
                let stdlib_source = core.load_stdlib(&compiling);
                // The boot is complete, so this is the state the next start
                // hydrates instead of repeating (docs/impl/image/boot.md).
                let stored = policy.and_then(|image| {
                    let t = std::time::Instant::now();
                    let (heap, symbols, cctx) = core.dump_parts();
                    let stored = crate::image::boot::store(heap, symbols, cctx, &image);
                    crate::phase!(crate::trace::boot(), "boot", t, "image-store");
                    stored
                });
                (stdlib_source, BootSource::Compiled, stored.map(Arc::new))
            }
            None => (StdlibSource::Compiled, BootSource::Compiled, None),
        };
        core.vm().set_worker_boot(WorkerBoot {
            image: kept,
            stdlib: cache,
        });
        Booted {
            core,
            stdlib_source,
            boot_source,
        }
    }

    /// Build a core out of the first of `images` this binary accepts, or out
    /// of the three sources when it accepts none (docs/impl/image/boot.md).
    ///
    /// Answers the core, the hydrated image and the open image it came from,
    /// because the caller's stdlib step installs out of the same image the
    /// core's compile context did, and its workers hydrate the same descriptor.
    fn for_boot(
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

    /// Install a hydrated image's stdlib exports, in place of compiling and
    /// running stdlib.lisp. The image's region is already a process root, so
    /// this roots nothing of its own (docs/impl/image/boot.md).
    fn install_stdlib_from_image(&mut self, boot: &crate::image::boot::Boot) {
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
    pub fn load_stdlib(&mut self, cache: &StdlibCache) -> StdlibSource {
        // Record it on the VM so a `sys/spawn` worker, which reaches the
        // spawning instance only through `ctx.vm()`, inherits this directory
        // instead of falling back to the process-wide one.
        self.vm.set_worker_boot(WorkerBoot {
            image: None,
            stdlib: cache.clone(),
        });
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
    /// on one thread each get their own.
    pub fn heap(&mut self) -> &mut crate::value::fiberheap::FiberHeap {
        &mut self.heap
    }

    /// The three disjoint borrows the pipeline needs at once: the VM (execution),
    /// the symbol table (interning/resolution, shared with execution), and the
    /// compile context (macro expansion, meta, projections).
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
