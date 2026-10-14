// audited: 2026-10-14
//! `CompileCtx`: one instance's compile-time state.
//!
//! src/pipeline/AGENTS.md
//!
//! A macro-expansion VM, the prelude/core `Expander`, the `PrimitiveMeta`
//! (primitives + core.lisp + stdlib exports + host bindings), the REPL layer,
//! and the capabilities the fiber a compile runs for withholds.
//!
//! This is owned by the instance's `RuntimeCore` (a sibling of the `VM` and
//! `SymbolTable`) and threaded explicitly through the pipeline: two embedded Elle
//! instances on one thread each own their own `CompileCtx`, so instance A's stdlib
//! exports and REPL `def`s are invisible to instance B.

use crate::hir::typeinfer::{DispatchWrapperRegistry, FnInlineRegistry, Registries};
use crate::primitives::def::PrimitiveMeta;
use crate::primitives::{build_primitive_meta, register_primitives};
use crate::signals::Signal;
use crate::symbol::SymbolTable;
use crate::syntax::{Expander, UnitTag};
use crate::value::arena::RootRef;
use crate::value::fiber::SignalBits;
use crate::vm::VM;
use std::collections::HashMap;

use super::bootstrap::{compile_core, install_core_exports};

mod registries;
mod repl;
use registries::InstanceRegistries;
pub use repl::Layer;
use repl::ReplLayer;

/// The two export structs a boot leaves behind: core.lisp's and stdlib.lisp's.
///
/// Both are process roots already, so holding them costs nothing; the boot
/// dump is what reads them, because they are the root set an image carries
/// (docs/impl/image/boot.md). Nil until the boot that produces each one runs.
#[derive(Debug, Clone, Copy)]
pub struct BootExports {
    pub core: crate::value::Value,
    pub stdlib: crate::value::Value,
}

impl Default for BootExports {
    fn default() -> Self {
        BootExports {
            core: crate::value::Value::NIL,
            stdlib: crate::value::Value::NIL,
        }
    }
}

/// Per-instance compile-time state.
///
/// # Invariants
///
/// - The macro-expansion VM's fiber is always reset between uses.
/// - The `Expander` is cloned for each pipeline call (independent expansion
///   state); its `eval_meta` (an `Rc`) is a cheap pointer-bump clone.
/// - Primitive registration order is deterministic (`ALL_TABLES`), so the
///   `SymbolId`s baked into `meta` match any `SymbolTable` that interned the
///   primitives in the same order — including the owning instance's table.
pub struct CompileCtx {
    /// VM with primitives registered, used only to evaluate macro bodies.
    /// Fiber always reset between uses.
    vm: VM,
    /// Expander with core.lisp `core_env` and the prelude macros loaded.
    /// Carries `eval_meta` (primitives + stdlib) so `eval_syntax` can compile
    /// macro bodies without a separate `CompileCtx` borrow.
    expander: Expander,
    /// Primitive metadata: primitives + core.lisp exports + stdlib exports +
    /// host bindings. The analyzer's `bind_primitives` reads this so user
    /// code sees them as immutable globals; `lookup_stdlib_value` reads it for
    /// the runtime `ev/run` entry.
    meta: PrimitiveMeta,
    /// The macros and definitions earlier REPL lines made. Only a compile of
    /// [`Layer::Repl`] sees them.
    repl: ReplLayer,
    /// The capabilities the fiber this compile runs for withholds, set by
    /// [`on_behalf_of`](Self::on_behalf_of). The macro VM's fresh fiber
    /// withholds them too, so a transformer spends no more than the code that
    /// started the compile could.
    withheld: SignalBits,
    /// The cross-unit registries the standard library's compile fills and
    /// every later compile reads.
    registries: InstanceRegistries,
    /// The core and stdlib export aggregates this instance booted with.
    exports: BootExports,
}

impl CompileCtx {
    /// Build a fresh compile context on a standalone macro VM (its own
    /// thread-root heap). For pipeline/test use where the context has no owning
    /// `RuntimeCore`.
    pub fn new() -> Self {
        Self::on_vm(VM::new())
    }

    /// Build a compile context whose macro-expansion VM shares an
    /// externally-owned heap (`RuntimeCore`'s). core.lisp's exports are runtime
    /// closures created here on the macro VM; sharing the instance heap is what
    /// lets the program VM resolve and call them without a cross-heap reference.
    pub fn new_with_heap(heap_ptr: *mut crate::value::fiberheap::FiberHeap) -> Self {
        Self::on_vm(VM::new_with_heap(heap_ptr))
    }

    /// The macro VM's heap pointer — the instance heap when this context was
    /// built with [`new_with_heap`](Self::new_with_heap). The compile pipeline
    /// allocates its per-compilation transient scratch into this heap so the
    /// scratch and the macro expander's allocations share one region store.
    pub fn heap_ptr(&self) -> *mut crate::value::fiberheap::FiberHeap {
        self.vm.heap_ptr
    }

    /// The Unicode generation this instance compiles under. Stored on the
    /// macro VM (one source of truth per instance): macro-time string ops
    /// and the analyzer's `(unicode! …)` check read the same value the
    /// program VM runs with.
    pub fn unicode_generation(&self) -> crate::segment::Generation {
        self.vm.unicode_generation()
    }

    /// Select the generation. Construction-time only, set by the owning
    /// `RuntimeCore` before any compile runs.
    pub(crate) fn set_unicode_generation(&mut self, gen: crate::segment::Generation) {
        self.vm.set_unicode_generation(gen);
    }

    /// Shared compile-context construction over an already-built macro VM
    /// (standalone or instance-heap-sharing).
    fn on_vm(mut vm: VM) -> Self {
        let boot = crate::trace::boot();
        let mut init_symbols = SymbolTable::new();
        let t = std::time::Instant::now();
        let mut meta = register_primitives(&mut vm, &mut init_symbols);
        // The expander's template arena is this macro VM's heap — the
        // instance's, when built through `new_with_heap`.
        let mut expander = Expander::on_vm(&mut vm);
        // Macro-transformer bodies compile against primitives (+ stdlib once
        // `init_stdlib` runs); seed it before `load_prelude`, whose macro
        // expansions evaluate transformer bodies via `eval_syntax`.
        expander.set_eval_meta(build_primitive_meta(&mut init_symbols));
        crate::phase!(boot, "boot", t, "primitives-macrovm");
        let t = std::time::Instant::now();
        let core = compile_core(&mut vm, &mut init_symbols, &mut meta, &mut expander);
        crate::phase!(boot, "boot", t, "core");
        let t = std::time::Instant::now();
        expander
            .load_prelude(&mut init_symbols, &mut vm)
            .expect("prelude loading must succeed");
        crate::phase!(boot, "boot", t, "prelude");
        // `init_symbols` is a throwaway used only for this setup; `expand` pointed
        // the macro VM at it. Reset to null so the dropped table is never reached
        // — the next `expand` (a real compile) re-points the VM at the instance's
        // table (docs/impl/region/ctx.md § "Symbols").
        vm.set_symbols(std::ptr::null_mut());
        Self::assemble(vm, expander, meta, core)
    }

    /// Build a compile context out of a hydrated boot image instead of
    /// compiling core.lisp and prelude.lisp (docs/impl/image/boot.md).
    ///
    /// The primitives still register on the macro VM, because a native-fn is a
    /// process-local id no image carries; everything else is installed from the
    /// image. `symbols` is the instance's own table, which the hydration has
    /// already taught the image's spellings — the throwaway table `on_vm` uses
    /// would know none of them.
    pub(crate) fn from_boot_image(
        heap_ptr: *mut crate::value::fiberheap::FiberHeap,
        boot: &crate::image::boot::Boot,
        symbols: &mut SymbolTable,
    ) -> Self {
        let trace = crate::trace::boot();
        let t = std::time::Instant::now();
        let mut vm = VM::new_with_heap(heap_ptr);
        let mut init_symbols = SymbolTable::new();
        let mut meta = register_primitives(&mut vm, &mut init_symbols);
        let mut expander = Expander::on_vm(&mut vm);
        expander.set_eval_meta(build_primitive_meta(&mut init_symbols));
        crate::phase!(trace, "boot", t, "primitives-macrovm");
        let t = std::time::Instant::now();
        install_core_exports(boot.core_exports(), symbols, &mut meta, &mut expander);
        boot.install_macros(unsafe { &mut *heap_ptr }, &mut expander, symbols);
        crate::phase!(trace, "boot", t, "image-core-and-macros");
        vm.set_symbols(std::ptr::null_mut());
        Self::assemble(vm, expander, meta, boot.core_exports())
    }

    /// The context a boot leaves: its macro VM, expander and meta, and the
    /// core.lisp exports. Every other part starts empty.
    fn assemble(
        vm: VM,
        expander: Expander,
        meta: PrimitiveMeta,
        core: crate::value::Value,
    ) -> Self {
        CompileCtx {
            vm,
            expander,
            meta,
            repl: ReplLayer::default(),
            withheld: SignalBits::EMPTY,
            registries: InstanceRegistries::default(),
            exports: BootExports {
                core,
                ..BootExports::default()
            },
        }
    }

    /// The instance's cross-unit registries as the compile now running sees
    /// them: recording inside [`compiling_stdlib`](Self::compiling_stdlib), and
    /// reading everywhere else (docs/impl/dissolution/inline.md).
    pub fn registries(&mut self) -> Registries<'_> {
        self.registries.view()
    }

    /// Run `f`, the standard library's compile, so that it records its
    /// definitions into the instance's cross-unit registries.
    pub fn compiling_stdlib<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let saved = std::mem::replace(&mut self.registries.stdlib_compile, true);
        let out = f(self);
        self.registries.stdlib_compile = saved;
        out
    }

    /// Both registries, for the stdlib disk cache: it stores what the standard
    /// library's compile recorded, and restores that on a hit.
    pub(crate) fn stdlib_registries_mut(
        &mut self,
    ) -> (&mut DispatchWrapperRegistry, &mut FnInlineRegistry) {
        (&mut self.registries.dispatch, &mut self.registries.inline)
    }

    /// Run `f` with the macro-expansion VM (fiber reset), a clone of the
    /// `Expander` (independent expansion state), and a clone of the compile
    /// `meta`, each of `layer`. The clones decouple `f` from `self`'s borrow so
    /// a nested compile during `f` (a `begin-for-syntax` that imports, say)
    /// does not alias. The fresh fiber withholds what
    /// [`on_behalf_of`](Self::on_behalf_of) named.
    ///
    /// `arena` is the unit's working syntax arena, and the handed-out clone is
    /// already pointed at it. Taking it here rather than leaving it to the
    /// caller is what keeps an expansion from allocating into the instance's
    /// process-root template arena, which nothing would ever reclaim
    /// (docs/impl/syntax.md § "Where a node lives"). `unit` is the unit the
    /// compile expands, and the clone has begun it, so every scope it mints
    /// names that unit.
    pub fn with_macro_expansion<F, R>(
        &mut self,
        arena: crate::syntax::SyntaxArena,
        layer: Layer,
        unit: UnitTag,
        f: F,
    ) -> R
    where
        F: FnOnce(&mut VM, Expander, PrimitiveMeta) -> R,
    {
        self.vm.reset_fiber();
        self.vm.fiber.withheld = self.withheld;
        let (expander, meta) = self.layered(layer, arena, unit);
        f(&mut self.vm, expander, meta)
    }

    /// A cloned `Expander` — pointed at `arena` and begun on `unit`, as in
    /// [`with_macro_expansion`](Self::with_macro_expansion) — and compile
    /// `meta`, of the instance's layer, without borrowing the macro VM. Used by
    /// `eval`/`analyze`, which run expansion on their own VM.
    pub fn expander_and_meta(
        &self,
        arena: crate::syntax::SyntaxArena,
        unit: UnitTag,
    ) -> (Expander, PrimitiveMeta) {
        self.layered(Layer::Instance, arena, unit)
    }

    /// Clones of the expander and the meta a compile of `layer` sees, the
    /// expander pointed at `arena` and begun on `unit`.
    fn layered(
        &self,
        layer: Layer,
        arena: crate::syntax::SyntaxArena,
        unit: UnitTag,
    ) -> (Expander, PrimitiveMeta) {
        let mut expander = self.expander.clone();
        expander.set_arena(arena);
        expander.begin_unit(unit);
        let mut meta = self.meta.clone();
        if layer == Layer::Repl {
            self.repl.overlay(&mut expander, &mut meta);
        }
        (expander, meta)
    }

    /// The capabilities the fiber this compile runs for withholds.
    pub fn withheld(&self) -> SignalBits {
        self.withheld
    }

    /// Run `f`, a compile that code on a fiber started, with the capabilities
    /// that fiber withholds. Every macro the compile expands on the macro VM
    /// runs without them, and an include reads no file without `:fs`. A
    /// compile inside another keeps what the outer one withheld.
    pub fn on_behalf_of<R>(&mut self, withheld: SignalBits, f: impl FnOnce(&mut Self) -> R) -> R {
        let saved = self.withheld;
        self.withheld = saved.union(withheld);
        let out = f(self);
        self.withheld = saved;
        out
    }

    /// Every globally-bound callable's `SymbolId`: Rust primitives, core.lisp
    /// exports, and stdlib exports (e.g. `+`, which is a stdlib closure over the
    /// `%add` intrinsic, not a primitive — so it is absent from `vm.docs`). The
    /// LSP uses this as the authoritative builtin-completion name set.
    pub fn global_function_ids(&self) -> impl Iterator<Item = crate::value::SymbolId> + '_ {
        self.meta.functions.keys().copied()
    }

    /// Look up a stdlib-exported (or host-bound) value by `SymbolId`. The
    /// runtime `ev/run` entry resolves the scheduler closure this way.
    pub fn lookup_stdlib_value(
        &self,
        sym_id: crate::value::SymbolId,
    ) -> Option<crate::value::Value> {
        self.meta.functions.get(&sym_id).copied()
    }

    /// This instance's macro table: every prelude macro, and every macro a
    /// boot image installed. The boot dump reads it, and so does
    /// the pin that a hydrated table carries the same entries
    /// (docs/impl/image/boot.md).
    pub(crate) fn macros(&self) -> &HashMap<String, crate::syntax::MacroDef> {
        self.expander.macros()
    }

    /// The core and stdlib export aggregates this instance booted with — the
    /// root set a boot image carries (docs/impl/image/boot.md).
    pub(crate) fn boot_exports(&self) -> BootExports {
        self.exports
    }

    /// Record the stdlib export aggregate, whatever produced it.
    pub(crate) fn set_stdlib_exports(&mut self, exports: crate::value::Value) {
        self.exports.stdlib = exports;
    }

    /// The core.lisp exports (name → Value), used to seed the expander's
    /// `core_env` when evaluating macro bodies that reference core functions.
    pub fn core_env(&self) -> HashMap<String, crate::value::Value> {
        self.expander.core_env.clone()
    }

    /// The full compile metadata user code is analyzed against: primitives,
    /// core.lisp exports, stdlib exports and host bindings.
    pub fn meta(&self) -> &PrimitiveMeta {
        &self.meta
    }

    /// The primitive(+stdlib) metadata for lowering's `PrimitiveClassification`
    /// and for macro-body compilation. Excludes core.lisp exports and every
    /// binding a host or a REPL line registers.
    pub fn primitive_meta(&self) -> &PrimitiveMeta {
        self.expander.eval_meta()
    }

    /// Register a binding an embedder makes, so that every compile in this
    /// instance resolves it, the files a program imports included. The value
    /// is rooted as [`register_repl_binding`](Self::register_repl_binding)
    /// roots one.
    pub fn register_host_binding(
        &mut self,
        heap: &mut crate::value::fiberheap::FiberHeap,
        sym_id: crate::value::SymbolId,
        value: crate::value::Value,
        funding: RootRef,
        signal: Signal,
        arity: Option<crate::value::types::Arity>,
    ) {
        self.meta.signals.insert(sym_id, signal);
        self.meta.functions.insert(sym_id, value);
        if let Some(a) = arity {
            self.meta.arities.insert(sym_id, a);
        }
        crate::value::arena::register_process_root(heap, value, funding);
    }

    /// Register a REPL `def` binding so later REPL lines resolve it. No other
    /// compile does: a file a line imports compiles as it would from any
    /// program.
    ///
    /// A REPL `def` value outlives the line that produced it. Under the
    /// mint-at-return convention the top-level return mint's +1 is balanced by
    /// the caller's decref at the result's decref_point, so without a root the
    /// value would be freed at the end of its line; register the value's region
    /// as a process root to keep it live for the session and release it by RC
    /// at teardown.
    ///
    /// `funding` says which reference that root is made of
    /// (docs/impl/region/rules.md § "The program value is the host's to
    /// release"): a caller holding the value's own reference hands it over with
    /// `RootRef::Take`, and one registering a value it reaches through another
    /// (a leaf of a destructuring `def`) asks for `RootRef::Mint`. A minted root
    /// funds itself per registration, so two leaves that share one region mint
    /// two references against the sweep's two decrefs. A taken root does not:
    /// the caller holds one reference, so it registers the region once (R9).
    pub fn register_repl_binding(
        &mut self,
        heap: &mut crate::value::fiberheap::FiberHeap,
        sym_id: crate::value::SymbolId,
        value: crate::value::Value,
        funding: RootRef,
        signal: Signal,
        arity: Option<crate::value::types::Arity>,
    ) {
        self.repl.bind(sym_id, value, signal, arity);
        crate::value::arena::register_process_root(heap, value, funding);
    }

    /// Register a host primitive under its name and aliases, so compiled code
    /// and `eval` both resolve it as they resolve a built-in.
    ///
    /// A host binding reaches the compile `meta` alone, and `eval` compiles
    /// against the macro-body `eval_meta`; a primitive belongs in both. The
    /// macro VM's doc table learns it too, so a macro body reads it as run
    /// time does (docs/macros.md). A primitive value is an immediate, so
    /// nothing here needs a root.
    pub fn register_primitive(
        &mut self,
        symbols: &mut SymbolTable,
        def: &'static crate::primitives::def::PrimitiveDef,
    ) {
        let mut eval_meta = self.expander.eval_meta().clone();
        for name in def.names() {
            let sym_id = symbols.intern(name);
            self.meta.insert_def(sym_id, def);
            eval_meta.insert_def(sym_id, def);
        }
        self.expander.set_eval_meta(eval_meta);
        crate::primitives::def::Doc::enter(&mut self.vm.docs, def);
    }

    /// Keep the macros a REPL line defined, from the expander that compiled
    /// it, so later REPL lines expand them. The instance's own macros in that
    /// expander are not the line's, and stay where they are.
    pub fn register_repl_macros(&mut self, macros: &HashMap<String, crate::syntax::MacroDef>) {
        self.repl.keep_macros(self.expander.macros(), macros);
    }

    /// Add stdlib exports to the compile `meta` (so user code sees them as
    /// globals) and to the macro-body `eval_meta` (so transformer bodies can
    /// call stdlib). Called by `init_stdlib` after executing stdlib.lisp.
    pub fn register_stdlib_exports(
        &mut self,
        exports: &HashMap<crate::value::SymbolId, (crate::value::Value, Signal)>,
    ) {
        for (sym_id, (value, signal)) in exports {
            self.meta.signals.insert(*sym_id, *signal);
            self.meta.functions.insert(*sym_id, *value);
        }
        // Mirror into the macro-body metadata (primitives + stdlib).
        let mut eval_meta = self.expander.eval_meta().clone();
        for (sym_id, (value, signal)) in exports {
            eval_meta.signals.insert(*sym_id, *signal);
            eval_meta.functions.insert(*sym_id, *value);
        }
        self.expander.set_eval_meta(eval_meta);
    }

    /// Release what this context holds on the heap from Rust, ahead of the
    /// process-teardown sweep. Each macro transformer's cell would release on
    /// its own when the context drops, which is after the sweep has counted the
    /// residue. So the master expander and the REPL layer empty their cells
    /// here, and the macro VM drops the `eval` expander a macro body may have
    /// built.
    ///
    /// The macro VM's JIT and SPIR-V caches pin code regions from Rust, so they
    /// go here too (docs/impl/jit.md).
    pub fn release(&mut self) {
        self.expander.release_cached_transformers();
        self.repl.release();
        self.vm.eval_expander = None;
        self.vm.clear_code_pins();
    }
}

impl Default for CompileCtx {
    fn default() -> Self {
        Self::new()
    }
}
