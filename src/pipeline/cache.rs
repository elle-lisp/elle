//! `CompileCtx`: the per-instance compile-time state — a macro-expansion VM, the
//! prelude/core `Expander`, the `PrimitiveMeta` (primitives + core.lisp + stdlib
//! exports + REPL value bindings), and the file→signal projection map.
//!
//! This is owned by the instance's `RuntimeCore` (a sibling of the `VM` and
//! `SymbolTable`) and threaded explicitly through the pipeline: two embedded Elle
//! instances on one thread each own their own `CompileCtx`, so instance A's stdlib
//! exports and REPL `def`s are invisible to instance B.

use crate::hir::typeinfer::{DispatchWrapperRegistry, FnInlineRegistry};
use crate::primitives::def::PrimitiveMeta;
use crate::primitives::{build_primitive_meta, register_primitives};
use crate::signals::Signal;
use crate::symbol::SymbolTable;
use crate::syntax::Expander;
use crate::vm::VM;
use std::collections::HashMap;

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
    /// REPL value bindings. The analyzer's `bind_primitives` reads this so user
    /// code sees them as immutable globals; `lookup_stdlib_value` reads it for
    /// the runtime `ev/run` entry.
    meta: PrimitiveMeta,
    /// Signal projection cache: resolved file path → keyword→signal projection.
    /// Populated lazily when the analyzer encounters `(import "...")` with a
    /// literal string argument. Per-instance, though projections are in fact
    /// deterministic from file content (an instance never shares one).
    projections: HashMap<String, Option<HashMap<String, Signal>>>,
    /// Container-dispatch wrappers collected across every compile in this
    /// instance, keyed by name. Populated when `stdlib.lisp` compiles (its
    /// `push`/`put`), consumed by every later unit so a user→stdlib wrapper call
    /// monomorphizes as an intra-unit one does (the F1b close, `monomorphize.rs`).
    /// Compile-time-only state: it drives an HIR rewrite and never reaches the VM.
    dispatch_wrappers: DispatchWrapperRegistry,
    /// Cross-unit-inlineable function templates collected across every compile in
    /// this instance, keyed by name. Populated when `stdlib.lisp` compiles (its
    /// `inc`/`dec`/… bodies), consumed by every later unit so a user→stdlib
    /// `(map inc xs)` inlines the stdlib body as a same-unit named fn would (the
    /// dissolution leg across the compile-unit boundary, `fuse.rs`). Like
    /// `dispatch_wrappers`, compile-time-only state that never reaches the VM.
    fn_inline: FnInlineRegistry,
}

/// core.lisp source, embedded at compile time.
const CORE: &str = include_str!("../core.lisp");

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
    /// lets the program VM resolve and call them without a cross-heap reference
    /// (tls.md § the ownership flip).
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
        let mut init_symbols = SymbolTable::new();
        let mut meta = register_primitives(&mut vm, &mut init_symbols);
        let mut expander = Expander::new();
        // Macro-transformer bodies compile against primitives (+ stdlib once
        // `init_stdlib` runs); seed it before `load_prelude`, whose macro
        // expansions evaluate transformer bodies via `eval_syntax`.
        expander.set_eval_meta(build_primitive_meta(&mut init_symbols));
        compile_core(&mut vm, &mut init_symbols, &mut meta, &mut expander);
        expander
            .load_prelude(&mut init_symbols, &mut vm)
            .expect("prelude loading must succeed");
        // `init_symbols` is a throwaway used only for this setup; `expand` pointed
        // the macro VM at it. Reset to null so the dropped table is never reached
        // — the next `expand` (a real compile) re-points the VM at the instance's
        // table (docs/impl/region/ctx.md § "Symbols").
        vm.set_symbols(std::ptr::null_mut());
        CompileCtx {
            vm,
            expander,
            meta,
            projections: HashMap::new(),
            dispatch_wrappers: DispatchWrapperRegistry::default(),
            fn_inline: FnInlineRegistry::default(),
        }
    }

    /// The instance's two cross-unit compile registries, borrowed together (they
    /// are disjoint fields, so one accessor yields both `&mut` without aliasing —
    /// `regularize` needs both, and two separate accessor calls would each borrow
    /// all of `self`). The `<stdlib>` compile populates both; later user compiles
    /// consult them. `dispatch_wrappers` drives container-dispatch monomorphization
    /// (`monomorphize.rs`); `fn_inline` drives cross-unit HOF-argument inlining
    /// (`fuse.rs`).
    pub fn compile_registries_mut(
        &mut self,
    ) -> (&mut DispatchWrapperRegistry, &mut FnInlineRegistry) {
        (&mut self.dispatch_wrappers, &mut self.fn_inline)
    }

    /// Run `f` with the macro-expansion VM (fiber reset), a clone of the
    /// `Expander` (independent expansion state), and a clone of the compile
    /// `meta`. The clones decouple `f` from `self`'s borrow so a nested compile
    /// during `f` (a `begin-for-syntax` that imports, say) does not alias.
    pub fn with_macro_expansion<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut VM, Expander, PrimitiveMeta) -> R,
    {
        self.vm.reset_fiber();
        let expander = self.expander.clone();
        let meta = self.meta.clone();
        f(&mut self.vm, expander, meta)
    }

    /// A cloned `Expander` and compile `meta` without borrowing the macro VM.
    /// Used by `eval`/`analyze`, which run expansion on their own VM.
    pub fn expander_and_meta(&self) -> (Expander, PrimitiveMeta) {
        (self.expander.clone(), self.meta.clone())
    }

    /// Every globally-bound callable's `SymbolId`: Rust primitives, core.lisp
    /// exports, and stdlib exports (e.g. `+`, which is a stdlib closure over the
    /// `%add` intrinsic, not a primitive — so it is absent from `vm.docs`). The
    /// LSP uses this as the authoritative builtin-completion name set.
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

    /// The core.lisp exports (name → Value), used to seed the expander's
    /// `core_env` when evaluating macro bodies that reference core functions.
    pub fn core_env(&self) -> HashMap<String, crate::value::Value> {
        self.expander.core_env.clone()
    }

    /// The primitive(+stdlib) metadata for lowering's `PrimitiveClassification`
    /// and for macro-body compilation. Excludes core.lisp exports and REPL
    /// value bindings.
    pub fn primitive_meta(&self) -> &PrimitiveMeta {
        self.expander.eval_meta()
    }

    /// Register a REPL `def` binding so subsequent compilations resolve it.
    ///
    /// A REPL `def` value outlives the line that produced it — later lines
    /// resolve it from `meta`. Under the mint-at-return convention the top-level
    /// return mint's +1 is balanced by the caller's decref at the result's
    /// decref_point, so without a root the value would be freed at the end of its
    /// line; register the value's region as a process root to keep it live for
    /// the session and release it by RC at teardown. Each `def` (including a
    /// redefinition) is a distinct fresh region, so one registration per call
    /// does not double-decref (R9).
    pub fn register_repl_binding(
        &mut self,
        heap: &mut crate::value::fiberheap::FiberHeap,
        sym_id: crate::value::SymbolId,
        value: crate::value::Value,
        signal: Signal,
        arity: Option<crate::value::types::Arity>,
    ) {
        self.meta.signals.insert(sym_id, signal);
        self.meta.functions.insert(sym_id, value);
        if let Some(a) = arity {
            self.meta.arities.insert(sym_id, a);
        }
        crate::value::arena::register_process_root(heap, value);
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

    /// Release the region reference each pre-compiled macro transformer holds.
    /// Part of the process-teardown sweep: those transformer closure `Value`s
    /// are `Copy`, so a plain drop would never decref them and they would survive
    /// teardown as residue. The `CompileCtx` is this instance's sole holder, so
    /// the decref is balanced. Run while the heap is still alive (before drop).
    pub fn release(&mut self, heap: &mut crate::value::fiberheap::FiberHeap) {
        self.expander.release_cached_transformers(heap);
    }

    /// Look up or compute the signal projection for an imported file.
    ///
    /// On a miss, compiles the file (with a throwaway `SymbolTable`, in THIS
    /// instance's context) and caches the projection from the resulting
    /// bytecode. Returns `None` if the file's return value is not a projectable
    /// struct (cached as `None` to avoid recompiling).
    pub fn get_or_compile_projection(
        &mut self,
        resolved_path: &str,
        symbols: &mut SymbolTable,
    ) -> Option<HashMap<String, Signal>> {
        if let Some(proj) = self.projections.get(resolved_path) {
            return proj.clone();
        }

        let source = std::fs::read_to_string(resolved_path).ok()?;
        // Compile in the CALLER's symbol table, not a throwaway one. Macro
        // transformers compile lazily on first expansion and cache on the
        // expander; quoted-literal symbol ids baked into a transformer bind to
        // whichever table compiled it. A throwaway table here makes the FIRST
        // expansion (often this projection probe) bake throwaway ids, so the
        // real import's expansion compares against the instance table and
        // fails — e.g. `each`'s `(= (syntax->datum iter-or-in) 'in)`. The
        // stdlib disk cache skips the stdlib compile that would otherwise warm
        // every transformer; using the instance table keeps both paths consistent.
        let projection = super::compile::compile_file(&source, symbols, self, resolved_path)
            .ok()
            .and_then(|result| result.bytecode.signal_projection);

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

/// Compile and execute core.lisp, storing exports in the Expander's core_env.
///
/// Runs the full pipeline (read → expand → analyze → lower → emit → execute)
/// without using a `CompileCtx` (we're inside its construction). The bare
/// expander has no prelude macros — core.lisp uses only special forms and
/// %-prefixed intrinsics.
fn compile_core(
    vm: &mut VM,
    symbols: &mut SymbolTable,
    meta: &mut PrimitiveMeta,
    expander: &mut Expander,
) {
    use crate::hir::{Analyzer, BindingArena, FileForm};
    use crate::lir::{Emitter, Lowerer};
    use crate::primitives::intern_primitive_names;
    use crate::reader::read_syntax_all;
    use crate::syntax::Span;
    use std::rc::Rc;

    intern_primitive_names(symbols);

    let syntaxes = read_syntax_all(CORE, "<core>").expect("core.lisp parsing must succeed");

    // Expand with bare expander (no prelude)
    let mut bare_expander = Expander::new();
    let expanded_forms: Vec<_> = syntaxes
        .into_iter()
        .map(|s| bare_expander.expand(s, symbols, vm))
        .collect::<Result<_, _>>()
        .expect("core.lisp expansion must succeed");

    let forms: Vec<FileForm> = expanded_forms
        .iter()
        .map(crate::hir::classify_form)
        .collect();
    let span = if expanded_forms.is_empty() {
        Span::synthetic()
    } else {
        expanded_forms[0]
            .span
            .merge(&expanded_forms[expanded_forms.len() - 1].span)
    };

    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new_with_primitives(
        symbols,
        &mut arena,
        meta.signals.clone(),
        meta.arities.clone(),
    );
    analyzer.bind_primitives(meta);
    let mut hir = analyzer
        .analyze_file_letrec(forms, span)
        .expect("core.lisp analysis must succeed");
    let prim_values = analyzer.primitive_values().clone();
    let errors = analyzer.take_errors();
    drop(analyzer);

    if !errors.is_empty() {
        for e in &errors {
            eprintln!("core.lisp analysis error: {:?}", e);
        }
        panic!("core.lisp analysis produced {} error(s)", errors.len());
    }

    // core.lisp runs before the instance `CompileCtx` (and its registries) exists,
    // during `on_vm` construction, so throwaway registries are correct here: it
    // defines no container-dispatch wrappers (its `concat`/`reverse` fan to helpers,
    // not single monomorphic-op arms), and its cross-unit-inlineable fns are not
    // recorded for later units. The load-bearing cross-unit templates (`inc`/`dec`)
    // live in `stdlib.lisp`, which compiles through the instance registries.
    crate::hir::regularize(
        &mut hir,
        &mut arena,
        symbols,
        &mut DispatchWrapperRegistry::default(),
        &mut FnInlineRegistry::default(),
    )
    .expect("core.lisp uses no monomorphic container ops, so the proof obligation holds");

    let pc = crate::lir::intrinsics::PrimitiveClassification::new(symbols, meta);
    let region_info =
        crate::hir::analyze_regions_with(&hir, &arena, pc.call_classification.clone());
    if crate::config::get().trace_bits() & crate::config::trace_bits::REGIONS != 0 {
        let names = symbols.all_names();
        eprintln!(
            "[trace:regions] cache (core.lisp):\n{}",
            crate::hir::format_regions(&region_info, &arena, &names)
        );
    }
    let symbol_names = symbols.all_names();
    let mut lowerer = Lowerer::new(&arena)
        .with_primitive_classification(pc)
        .with_primitive_values(prim_values)
        .with_symbol_names(symbol_names.clone())
        .with_region_info(region_info);
    let lir_module = lowerer
        .lower(&hir)
        .expect("core.lisp lowering must succeed");

    let mut emitter = Emitter::new_with_symbols(symbol_names);
    let (bytecode, _yield_points, _call_sites) = emitter.emit_module(&lir_module);

    let closure_val = vm
        .execute(&bytecode)
        .expect("core.lisp execution must succeed");

    let closure = closure_val
        .as_closure()
        .expect("core.lisp must return a closure");
    let env = Rc::new(crate::primitives::module_init::build_closure_call_env(
        closure,
        &[],
    ));
    let exports_val = vm
        .execute_bytecode(
            &closure.template.bytecode,
            &closure.template.constants,
            &closure.template.child_protos,
            crate::value::code::CodeTables {
                merged_slots: closure.template.merged_slots.clone(),
                frame_release_slots: closure.template.frame_release_slots.clone(),
                frame_release_regions: closure.template.frame_release_regions.clone(),
            },
            Some(&env),
        )
        .expect("core.lisp export closure must succeed");

    // Root the core export aggregate, not each entry. `exports_val` is the
    // struct returned by core.lisp; it references every core export (each was
    // incref'd into the struct when built), and the per-name `Value`s copied
    // into `core_env`/`meta` below are aliases into those same regions. Under the
    // mint-at-return convention this struct survives on the top-level return
    // mint's +1, which the caller balances at the result's decref_point — so
    // without a root the struct would be freed there, cascade-freeing the exports
    // while `core_env` and `meta` still alias them (dangling reads on later
    // compiles). Registering the struct (and the module closure that produced it)
    // as process roots keeps the exports live for the process and lets the
    // teardown sweep reclaim them by RC cascade. One registration each — these
    // are distinct regions, so no double-decref (R9).
    crate::value::arena::register_process_root(unsafe { &mut *vm.heap_ptr }, closure_val);
    crate::value::arena::register_process_root(unsafe { &mut *vm.heap_ptr }, exports_val);

    let exports_struct = exports_val
        .as_struct()
        .expect("core.lisp must return a struct");
    for (key, value) in exports_struct.iter() {
        if let crate::value::types::TableKey::Keyword(name) = key {
            // core_env: name-keyed, used by eval_syntax for macro bodies
            expander.core_env.insert(name.to_string(), *value);
            // meta: SymbolId-keyed, used by compile_file for user code
            let sym_id = symbols.intern(name);
            let signal = if let Some(c) = value.as_closure() {
                c.template.signal
            } else {
                Signal::silent()
            };
            meta.signals.insert(sym_id, signal);
            meta.functions.insert(sym_id, *value);
        }
    }
}
