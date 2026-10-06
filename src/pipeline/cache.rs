// audited: 2026-10-20
//! `CompileCtx`, one instance's compile-time state, and `BootExports`, the export structs its boot left.
//!
//! src/pipeline/AGENTS.md
//! docs/impl/image/boot.md
//! docs/impl/region/rules.md
//!
//! The state is a macro-expansion VM, the prelude and core `Expander`, the
//! `PrimitiveMeta` (primitives, host primitives, core.lisp and stdlib exports,
//! REPL value bindings), and the map from an imported file to its signal projection.
//!
//! The instance's `RuntimeCore` owns it beside the `VM` and the `SymbolTable`,
//! and the pipeline takes it as a parameter. Two embedded Elle instances on one
//! thread each own a `CompileCtx`, so neither sees the other's stdlib exports
//! or REPL `def`s.

use crate::primitives::def::PrimitiveMeta;
use crate::primitives::{build_primitive_meta, register_primitives};
use crate::signals::Signal;
use crate::symbol::SymbolTable;
use crate::syntax::Expander;
use crate::value::arena::RootRef;
use crate::vm::VM;
use std::collections::HashMap;

use super::bootstrap::{compile_core, install_core_exports};

/// The two export structs a boot leaves behind: core.lisp's and stdlib.lisp's.
///
/// Both are process roots already, so holding them costs nothing. The boot
/// dump reads them, because they are the root set an image carries. Each is
/// nil until the boot that produces it runs.
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
/// - A `SymbolId` is its name's hash, so the ids that the setup's throwaway
///   `SymbolTable` baked into `meta` name the same symbols in the owning
///   instance's table.
pub struct CompileCtx {
    /// VM with primitives registered, used only to evaluate macro bodies.
    /// Fiber always reset between uses.
    vm: VM,
    /// Expander with core.lisp `core_env` and the prelude macros loaded.
    /// Carries `eval_meta` (primitives + stdlib) so `eval_syntax` can compile
    /// macro bodies without a separate `CompileCtx` borrow.
    expander: Expander,
    /// Primitive metadata: primitives + core.lisp exports + stdlib exports +
    /// REPL value bindings. The analyzer's `bind_primitives` reads this so user
    /// code sees them as immutable globals; `lookup_stdlib_value` reads it for
    /// the runtime `ev/run` entry.
    meta: PrimitiveMeta,
    /// Signal projection cache: resolved file path → keyword→signal projection.
    /// Populated lazily when the analyzer encounters `(import "...")` with a
    /// literal string argument. Per-instance, keyed by resolved path, and never
    /// invalidated: an edit to the file during the instance's life does not
    /// reach the cached projection.
    projections: HashMap<String, Option<HashMap<String, Signal>>>,
    /// The core and stdlib export aggregates this instance booted with.
    exports: BootExports,
    /// Whether a unit compiled here grafts its calls (docs/impl/graft.md).
    graft: crate::config::GraftPolicy,
    /// The static-region counter where this instance's boot ended, taken as
    /// the stdlib exports install. A boot image records it, so an image boot
    /// hands user code the slots a source boot does (docs/impl/image/format.md).
    static_region_mark: u32,
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

    /// Whether the units this context compiles from now on graft their calls
    /// (docs/impl/graft.md). The owning `Runtime` sets it after its boot, so the
    /// boot's own units always graft.
    pub(crate) fn set_graft(&mut self, graft: crate::config::GraftPolicy) {
        self.graft = graft;
    }

    /// Whether the units this context compiles graft their calls.
    pub(crate) fn graft(&self) -> crate::config::GraftPolicy {
        self.graft
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
        // `init_symbols` is a throwaway used only for this setup, and `expand`
        // pointed the macro VM at it. Reset the pointer to null so nothing reaches
        // the dropped table. The next `expand`, a real compile, points the VM at
        // the instance's table.
        vm.set_symbols(std::ptr::null_mut());
        CompileCtx {
            vm,
            expander,
            meta,
            projections: HashMap::new(),
            exports: BootExports {
                core,
                ..BootExports::default()
            },
            graft: crate::config::GraftPolicy::On,
            static_region_mark: 0,
        }
    }

    /// Build a compile context out of a hydrated boot image instead of
    /// compiling core.lisp and prelude.lisp.
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
        CompileCtx {
            vm,
            expander,
            meta,
            projections: HashMap::new(),
            exports: BootExports {
                core: boot.core_exports(),
                ..BootExports::default()
            },
            graft: crate::config::GraftPolicy::On,
            static_region_mark: 0,
        }
    }

    /// Run `f` with the macro-expansion VM (fiber reset), a clone of the
    /// `Expander` (independent expansion state), and a clone of the compile
    /// `meta`. The clones decouple `f` from `self`'s borrow so a nested compile
    /// during `f` (a `begin-for-syntax` that imports, say) does not alias.
    ///
    /// `arena` is the unit's working syntax arena, and the handed-out clone is
    /// already pointed at it. Taking it here rather than leaving it to the
    /// caller is what keeps an expansion from allocating into the instance's
    /// process-root template arena, which nothing would ever reclaim
    /// (docs/impl/syntax.md).
    pub fn with_macro_expansion<F, R>(&mut self, arena: crate::syntax::SyntaxArena, f: F) -> R
    where
        F: FnOnce(&mut VM, Expander, PrimitiveMeta) -> R,
    {
        self.vm.reset_fiber();
        let mut expander = self.expander.clone();
        expander.set_arena(arena);
        let meta = self.meta.clone();
        f(&mut self.vm, expander, meta)
    }

    /// A cloned `Expander` — pointed at `arena`, as in
    /// [`with_macro_expansion`](Self::with_macro_expansion) — and compile
    /// `meta`, without borrowing the macro VM. Used by `eval`/`analyze`, which
    /// run expansion on their own VM.
    pub fn expander_and_meta(
        &self,
        arena: crate::syntax::SyntaxArena,
    ) -> (Expander, PrimitiveMeta) {
        let mut expander = self.expander.clone();
        expander.set_arena(arena);
        (expander, self.meta.clone())
    }

    /// Every globally-bound callable's `SymbolId`: Rust primitives, core.lisp
    /// exports, and stdlib exports. The last include names absent from
    /// `vm.docs`, for example `+`, a stdlib closure over the `%add` intrinsic.
    /// The LSP uses this as the authoritative builtin-completion name set.
    pub fn global_function_ids(&self) -> impl Iterator<Item = crate::value::SymbolId> + '_ {
        self.meta.functions.keys().copied()
    }

    /// Look up a stdlib-exported (or REPL-bound) value by `SymbolId`. The
    /// runtime `ev/run` entry resolves the scheduler closure this way.
    pub fn lookup_stdlib_value(
        &self,
        sym_id: crate::value::SymbolId,
    ) -> Option<crate::value::Value> {
        self.meta.functions.get(&sym_id).copied()
    }

    /// This instance's macro table: every prelude macro, plus whatever the
    /// REPL and later compiles defined. The boot dump reads it, and so does
    /// the pin that a hydrated table carries the same entries.
    pub(crate) fn macros(&self) -> &HashMap<String, crate::syntax::MacroDef> {
        self.expander.macros()
    }

    /// The next hygiene scope id this instance's expander will mint. Read by
    /// the pin that an image boot mints the scopes a source boot would.
    #[cfg(test)]
    pub(crate) fn scope_counter(&self) -> u32 {
        self.expander.scope_counter()
    }

    /// The core and stdlib export aggregates this instance booted with, the
    /// root set a boot image carries.
    pub(crate) fn boot_exports(&self) -> BootExports {
        self.exports
    }

    /// Record the stdlib export aggregate, whatever produced it, and the
    /// static-region counter where that ends the boot.
    pub(crate) fn set_stdlib_exports(&mut self, exports: crate::value::Value) {
        self.exports.stdlib = exports;
        self.static_region_mark = crate::lir::lower::static_region_counter();
    }

    /// The static-region counter where this instance's boot ended, which the
    /// boot dump records (docs/impl/image/format.md).
    pub(crate) fn static_region_mark(&self) -> u32 {
        self.static_region_mark
    }

    /// The core.lisp exports (name → Value), used to seed the expander's
    /// `core_env` when evaluating macro bodies that reference core functions.
    pub fn core_env(&self) -> HashMap<String, crate::value::Value> {
        self.expander.core_env.clone()
    }

    /// The full compile metadata user code is analyzed against: primitives,
    /// core.lisp exports, stdlib exports and REPL value bindings.
    pub fn meta(&self) -> &PrimitiveMeta {
        &self.meta
    }

    /// The primitive(+stdlib) metadata for lowering's `PrimitiveClassification`
    /// and for macro-body compilation. Excludes core.lisp exports and REPL
    /// value bindings.
    pub fn primitive_meta(&self) -> &PrimitiveMeta {
        self.expander.eval_meta()
    }

    /// Register a REPL `def` binding so subsequent compilations resolve it.
    ///
    /// A REPL `def` value outlives the line that produced it, because later
    /// lines resolve it from `meta`. So the value's region becomes a process
    /// root, which keeps it live for the session and releases it by RC at
    /// teardown.
    ///
    /// `funding` says which reference that root is made of. A caller holding
    /// the value's own reference hands it over with `RootRef::Take`. A caller
    /// registering a value it reaches through another, such as a leaf of a
    /// destructuring `def`, asks for `RootRef::Mint`. A minted root funds
    /// itself per registration, so two leaves that share one region mint two
    /// references against the sweep's two decrefs. A taken root does not: the
    /// caller holds one reference, so it registers the region once.
    pub fn register_repl_binding(
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

    /// Register a host primitive under its name and aliases, so compiled code
    /// and `eval` both resolve it as they resolve a built-in.
    ///
    /// A REPL binding reaches the compile `meta` alone, and `eval` compiles
    /// against the macro-body `eval_meta`; a primitive belongs in both. A
    /// primitive value is an immediate, so nothing here needs a root.
    pub fn register_primitive(
        &mut self,
        symbols: &mut SymbolTable,
        def: &'static crate::primitives::def::PrimitiveDef,
    ) {
        let mut eval_meta = self.expander.eval_meta().clone();
        for name in std::iter::once(&def.name).chain(def.aliases) {
            let sym_id = symbols.intern(name);
            self.meta.insert_def(sym_id, def);
            eval_meta.insert_def(sym_id, def);
        }
        self.expander.set_eval_meta(eval_meta);
    }

    /// Merge REPL-defined macros into the expander so subsequent compilations
    /// see them. (The macro-body `eval_meta` is unaffected: REPL value bindings
    /// never reach macro-body compiles.)
    pub fn register_repl_macros(&mut self, macros: &HashMap<String, crate::syntax::MacroDef>) {
        self.expander.merge_macros(macros);
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
    /// residue. So the master expander empties its cells here, and the macro
    /// VM drops the `eval` expander a macro body may have built.
    ///
    /// The macro VM's JIT and SPIR-V caches pin code regions from Rust, so they
    /// go here too (docs/impl/jit.md).
    pub fn release(&mut self) {
        self.expander.release_cached_transformers();
        self.vm.eval_expander = None;
        self.vm.clear_code_pins();
    }

    /// Look up or compute the signal projection for an imported file.
    ///
    /// On a miss, compiles the file in this instance's context and caches the
    /// projection from the resulting bytecode. Returns `None` when the file
    /// cannot be read or compiled, or its return value is not a projectable
    /// struct; the `None` is cached too, so the file is not compiled again.
    pub fn get_or_compile_projection(
        &mut self,
        resolved_path: &str,
        symbols: &mut SymbolTable,
    ) -> Option<HashMap<String, Signal>> {
        if let Some(proj) = self.projections.get(resolved_path) {
            return proj.clone();
        }

        let source = std::fs::read_to_string(resolved_path).ok()?;
        // The probe compiles in the CALLER's memo, which is per-instance
        // (docs/impl/symbol.md). A throwaway table here would drop every name
        // the module's quoted data carries. A transformer cached on the shared
        // expander during this compile would also outlive the table it learned
        // into. The `each` and probe tests in
        // tests/integration/projection.rs pin both halves. A cache hit skips
        // the stdlib compile that would otherwise warm every transformer, so
        // the probe is often the first expansion on that path too.
        let projection = super::compile::compile_file(&source, symbols, self, resolved_path)
            .ok()
            .and_then(|unit| unit.signal_projection().cloned());

        self.projections
            .insert(resolved_path.to_string(), projection.clone());
        projection
    }
}

impl Default for CompileCtx {
    fn default() -> Self {
        Self::new()
    }
}
