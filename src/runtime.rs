// audited: 2026-10-21
//! The process runtime: one compile-and-run lifecycle and one teardown sweep, shared by every entry path.
//!
//! docs/impl/region/rules.md
//! docs/impl/image/boot.md
//!
//! A [`Runtime`] owns a [`RuntimeCore`], which holds the heap, the VM, the
//! symbol table and the compile context, and points the VM at the other three.
//! It boots them, from the boot image or from source. On `Drop` or an explicit
//! [`Runtime::teardown`] it runs the process teardown sweep that
//! docs/impl/region/rules.md specifies:
//!
//! 1. **RC-driven, never iterate-and-free.** It releases the registered process
//!    roots (each decref'd once) and lets the ordinary region RC cascade reclaim
//!    everything reachable. It never walks the region table freeing entries,
//!    because that would hide the leaks the sweep exists to show.
//! 2. **Observable.** It returns a [`TeardownReport`] naming the live region
//!    residue. The target is zero regions remaining.
//!
//! `elle foo.lisp`, the REPL and the embedding API all run this one routine, so
//! their teardown cannot drift. The naive user model holds: `elle foo.lisp` is
//! `(eval (wrap-in-letrec (read-all (slurp "foo.lisp"))))`, and afterward the
//! world returns to its pre-`main` state. Only the native-fn primitives
//! persist, and they are immediates that occupy no region.

mod boot;
mod core;

pub use boot::{BootCaches, WorkerBoot};
pub use core::RuntimeCore;

use crate::compiler::stdlib_cache::StdlibCache;
use crate::image::boot::{BootImage, BootSource};
use crate::pipeline::CompileCtx;
use crate::symbol::SymbolTable;
use crate::vm::VM;
pub(crate) use boot::{BootFrom, Booted};

/// The observable result of a teardown sweep (docs/impl/region/rules.md). The
/// standing target is `live_regions == 0`; a non-zero value is the current set
/// of open leaks — the remaining work, not a tuning knob.
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

/// The process runtime. Construct once per entry path; drop (or call
/// [`teardown`](Runtime::teardown)) to run the principled sweep.
pub struct Runtime {
    core: RuntimeCore,
    torn_down: bool,
    stdlib_source: crate::primitives::module_init::StdlibSource,
    boot_source: BootSource,
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

    /// Build a stdlib-loaded runtime whose VM segments strings under the
    /// given Unicode generation for its whole life. `Runtime::new()` uses
    /// the newest vendored generation (or the process `--unicode=` choice).
    pub fn with_unicode(gen: crate::segment::Generation) -> Self {
        Self::build_with(true, gen, BootCaches::process())
    }

    /// Build a stdlib-loaded runtime that boots from source and caches its
    /// compiled stdlib under `cache` rather than the process-wide directory.
    /// Two instances given the same directory share a cache; two given
    /// different ones cannot see each other's, which is what lets tests run
    /// beside each other. No boot image is read or stored, whatever the
    /// process-wide policy (docs/impl/image/boot.md).
    pub fn with_stdlib_cache(cache: StdlibCache) -> Self {
        Self::with_caches(BootCaches {
            stdlib: cache,
            image: BootImage::Off,
        })
    }

    /// Build a stdlib-loaded runtime over both cache policies: where its boot
    /// image lives, and where its compiled stdlib is cached.
    pub fn with_caches(caches: BootCaches) -> Self {
        Self::build_with(true, crate::config::get().unicode_generation(), caches)
    }

    fn build(load_stdlib: bool) -> Self {
        Self::build_with(
            load_stdlib,
            crate::config::get().unicode_generation(),
            BootCaches::process(),
        )
    }

    fn build_with(load_stdlib: bool, gen: crate::segment::Generation, caches: BootCaches) -> Self {
        let Booted {
            mut core,
            stdlib_source,
            boot_source,
        } = RuntimeCore::boot(gen, load_stdlib, BootFrom::Caches(caches));
        // The boot compiled under the graft. Every unit from here on compiles
        // under the process's policy (docs/impl/graft.md).
        core.compile().set_graft(crate::config::get().graft);

        // The post-boot heap census (docs/impl/image/measurements.md):
        // at this point every live object is boot state, the graph a boot
        // image must dump.
        if crate::trace::census() {
            for line in core.heap().census().lines() {
                eprintln!("[trace:census] {}", line);
            }
        }

        Runtime {
            core,
            torn_down: false,
            stdlib_source,
            boot_source,
        }
    }

    /// Where this instance's stdlib came from. A cache that silently never hits
    /// still yields a working runtime, so a test that only checks behavior
    /// cannot tell the two apart — this is what it asserts on instead.
    pub fn stdlib_source(&self) -> crate::primitives::module_init::StdlibSource {
        self.stdlib_source
    }

    /// Whether this instance hydrated a boot image or compiled its three boot
    /// sources. Reported for the reason [`stdlib_source`](Self::stdlib_source)
    /// is (docs/impl/image/boot.md).
    pub fn boot_source(&self) -> BootSource {
        self.boot_source
    }

    /// Dump this instance's boot state as a boot image at `path`, atomically.
    ///
    /// The graph is the core exports, the stdlib exports and the macro
    /// definitions, under a digest of the three sources they came from
    /// (docs/impl/image/boot.md). A refused value fails the dump before any
    /// byte is written.
    pub fn dump_boot_image(
        &mut self,
        path: &std::path::Path,
    ) -> Result<(), crate::image::ImageError> {
        let (heap, symbols, cctx) = self.core.dump_parts();
        crate::image::boot::dump(heap, symbols, cctx, path).map(drop)
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

    /// Register a host primitive on this instance. Compiled code and `eval`
    /// resolve it as a built-in, and `doc` answers for it at run time and in a
    /// macro body. The workers the instance spawns inherit all of that
    /// (docs/embedding.md).
    pub fn register_primitive(&mut self, def: &'static crate::primitives::def::PrimitiveDef) {
        let (vm, symbols, cctx) = self.parts();
        vm.register_host_primitive(symbols, cctx, def);
    }

    /// The compile context and this instance's heap as disjoint borrows — the
    /// pair an embedder hands to [`CompileCtx::register_host_binding`] (see
    /// [`RuntimeCore::compile_and_heap`]).
    pub fn compile_and_heap(
        &mut self,
    ) -> (&mut CompileCtx, &mut crate::value::fiberheap::FiberHeap) {
        self.core.compile_and_heap()
    }

    /// The heap and the symbol table as disjoint borrows — the pair the image
    /// dumper and hydrator take (see [`RuntimeCore::heap_and_symbols`]).
    pub fn heap_and_symbols(
        &mut self,
    ) -> (&mut crate::value::fiberheap::FiberHeap, &mut SymbolTable) {
        self.core.heap_and_symbols()
    }

    /// Run the process teardown sweep and return its observable report. RC-driven
    /// (roots released → cascade), never iterate-and-free. Idempotent: a second
    /// call releases nothing further (the registry was drained) and re-reports
    /// the residue.
    pub fn teardown(&mut self) -> TeardownReport {
        self.torn_down = true;

        // (1) Release the per-instance compile-time state's resident references.
        //     A macro transformer's cell holds a region reference, and it gives
        //     it back when its definition drops. The master expander and the
        //     program VM's `eval` expander would drop after the count below, so
        //     both release here. The `CompileCtx` itself (macro VM, expander,
        //     meta) drops with this core; the region pages its `Value`s alias
        //     are reclaimed by the RC sweep below.
        let heap_ptr = self.core.vm().heap_ptr;
        self.core.compile().release();
        self.core.vm().eval_expander = None;
        // The program VM's caches pin the code regions their keys name, from
        // Rust where the RC sweep cannot see them; drop the pins first, so no
        // pin holds a region past the sweep (docs/impl/jit.md).
        self.core.vm().clear_code_pins();
        // The default trait tables are `alloc_root`'d into this instance's root
        // region the RC sweep below releases; clear the heap's table so a later
        // read sees `NIL` instead of `Value`s pointing into the freed region.
        crate::primitives::traitregistry::reset_default_traits(unsafe { &mut *heap_ptr });

        // (2) The one heap action: release this instance's registered process
        //     roots by RC and let the cascade reclaim everything reachable.
        let roots_released =
            crate::value::arena::teardown_process_root_regions(unsafe { &mut *heap_ptr });
        // Every code unit and cache pin is a Rust handle on a code region; one
        // still live here outlives the regions the sweep just released
        // (docs/impl/region/template.md).
        debug_assert_eq!(
            unsafe { &*heap_ptr }.live_code_handles(),
            0,
            "a code unit or a cache pin is still live at teardown"
        );

        // (3) Observe the result. Every region is mortal, so every surviving
        //     region is leaked residue.
        let regions: Vec<(u32, u32, usize)> = unsafe { &*heap_ptr }.region_info_vec();

        // The teardown leak dump (docs/impl/region/diagnostics.md): each
        // survivor's `arena/dump` line, then the
        // cross-region edges among them — enough to compute externally-pinned
        // roots (rc > in-edges) and reference cycles offline.
        if crate::trace::residue() {
            let heap = unsafe { &*heap_ptr };
            eprintln!(
                "[trace:residue] live regions after teardown: {}",
                regions.len()
            );
            heap.debug_dump();
            for (referrer, referent) in heap.cross_ref_edges() {
                eprintln!("[trace:residue] edge {} -> {}", referrer, referent);
            }
        }

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
