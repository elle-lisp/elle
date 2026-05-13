//! Compilation cache: thread-local VM, Expander, PrimitiveMeta, and
//! signal projection cache.

use crate::primitives::def::PrimitiveMeta;
use crate::primitives::register_primitives;
use crate::signals::Signal;
use crate::symbol::SymbolTable;
use crate::syntax::Expander;
use crate::vm::VM;
use std::collections::HashMap;

/// Cached compilation state for pipeline functions.
///
/// Eliminates per-call costs of VM creation, primitive registration,
/// and prelude loading. Thread-local because VM contains Rc values.
///
/// # Invariants
///
/// - Prelude must be 100% defmacro (no runtime definitions)
/// - Primitives must be registered before any pipeline function call
/// - Pipeline functions are not re-entrant (no nested compile calls)
/// - Primitive registration order is deterministic (ALL_TABLES)
struct CompilationCache {
    /// VM with primitives registered. Fiber always reset between uses.
    vm: VM,
    /// Expander with prelude loaded. Cloned for each pipeline call.
    expander: Expander,
    /// Primitive metadata from register_primitives.
    meta: PrimitiveMeta,
    /// Signal projection cache: maps resolved file paths to their
    /// keyword→signal projections. Populated lazily when the analyzer
    /// encounters `(import "...")` with a literal string argument.
    projections: HashMap<String, Option<HashMap<String, Signal>>>,
}

/// core.lisp source, embedded at compile time.
const CORE: &str = include_str!("../core.lisp");

impl CompilationCache {
    fn new() -> Self {
        let mut vm = VM::new();
        let mut init_symbols = SymbolTable::new();
        let mut meta = register_primitives(&mut vm, &mut init_symbols);
        let mut expander = Expander::new();
        compile_core(&mut vm, &mut init_symbols, &mut meta, &mut expander);
        expander
            .load_prelude(&mut init_symbols, &mut vm)
            .expect("prelude loading must succeed");
        CompilationCache {
            vm,
            expander,
            meta,
            projections: HashMap::new(),
        }
    }
}

/// Compile and execute core.lisp, storing exports in the Expander's core_env.
///
/// Runs the full pipeline (read → expand → analyze → lower → emit → execute)
/// without using `with_compilation_cache` (we're inside cache initialization).
/// The bare expander has no prelude macros — core.lisp uses only special forms
/// and %-prefixed intrinsics.
fn compile_core(
    vm: &mut VM,
    symbols: &mut SymbolTable,
    meta: &mut PrimitiveMeta,
    expander: &mut Expander,
) {
    use crate::hir::functionalize::functionalize;
    use crate::hir::tailcall::mark_tail_calls;
    use crate::hir::{Analyzer, BindingArena, FileForm};
    use crate::lir::{Emitter, Lowerer};
    use crate::primitives::intern_primitive_names;
    use crate::reader::read_syntax_all;
    use crate::syntax::Span;
    use std::rc::Rc;

    intern_primitive_names(symbols);

    let syntaxes =
        read_syntax_all(CORE, "<core>").expect("core.lisp parsing must succeed");

    // Expand with bare expander (no prelude)
    let mut bare_expander = Expander::new();
    let expanded_forms: Vec<_> = syntaxes
        .into_iter()
        .map(|s| bare_expander.expand(s, symbols, vm))
        .collect::<Result<_, _>>()
        .expect("core.lisp expansion must succeed");

    let forms: Vec<FileForm> = expanded_forms
        .iter()
        .map(super::compile::classify_form)
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

    mark_tail_calls(&mut hir);
    functionalize(&mut hir, &mut arena);
    crate::hir::typeinfer::infer_and_rewrite(&mut hir, &arena, symbols);

    let pc = crate::lir::intrinsics::PrimitiveClassification::new(symbols);
    let region_info =
        crate::hir::analyze_regions_with(&hir, &arena, pc.call_classification.clone());
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
            Some(&env),
        )
        .expect("core.lisp export closure must succeed");

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

thread_local! {
    static COMPILATION_CACHE: std::cell::RefCell<Option<CompilationCache>> =
        const { std::cell::RefCell::new(None) };

    /// Signal projection cache: maps resolved file paths to their
    /// keyword→signal projections. Populated lazily when the analyzer
    /// encounters `(import "...")` with a literal string argument.
    static PROJECTION_CACHE: std::cell::RefCell<HashMap<String, Option<HashMap<String, Signal>>>> =
        std::cell::RefCell::new(HashMap::new());

    /// Escape projection cache: maps resolved file paths to their
    /// keyword→safe projections. Populated alongside signal projections.
    static ESCAPE_PROJECTION_CACHE: std::cell::RefCell<HashMap<String, Option<HashMap<String, crate::compiler::bytecode::FieldEscapeInfo>>>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Run a closure with access to the cached macro-expansion VM.
///
/// The VM's fiber is reset before each use. The Expander is cloned so
/// each call gets independent expansion state. The RefCell borrow is
/// held for the duration of `f`, so re-entrant calls will panic at the
/// borrow check — enforced by the type system, not convention.
pub(super) fn with_compilation_cache<F, R>(f: F) -> R
where
    F: FnOnce(&mut VM, Expander, PrimitiveMeta) -> R,
{
    COMPILATION_CACHE.with(|cache| {
        let mut cache_ref = cache.borrow_mut();
        let c = cache_ref.get_or_insert_with(CompilationCache::new);

        // Always reset fiber before use
        c.vm.reset_fiber();

        let expander = c.expander.clone();
        let meta = c.meta.clone();
        f(&mut c.vm, expander, meta)
    })
}

/// Get a cloned Expander and PrimitiveMeta from the cache without
/// borrowing the cached VM. Used by functions that have their own VM
/// (eval, analyze, analyze_file).
pub(super) fn get_cached_expander_and_meta() -> (Expander, PrimitiveMeta) {
    COMPILATION_CACHE.with(|cache| {
        let mut cache_ref = cache.borrow_mut();
        let c = cache_ref.get_or_insert_with(CompilationCache::new);
        (c.expander.clone(), c.meta.clone())
    })
}

/// Look up a stdlib-exported value by SymbolId from the compilation cache.
///
/// Returns the value if the symbol was registered via `update_cache_with_stdlib`.
pub fn lookup_stdlib_value(sym_id: crate::value::SymbolId) -> Option<crate::value::Value> {
    COMPILATION_CACHE.with(|cache| {
        cache
            .borrow()
            .as_ref()
            .and_then(|c| c.meta.functions.get(&sym_id).copied())
    })
}

/// Get the core.lisp exports (name → Value) from the compilation cache.
///
/// Used by the runtime eval instruction to seed the expander's core_env
/// so that macros referencing core functions (last, butlast, etc.) work.
pub fn get_cached_core_env() -> std::collections::HashMap<String, crate::value::Value> {
    COMPILATION_CACHE.with(|cache| {
        let mut cache_ref = cache.borrow_mut();
        let c = cache_ref.get_or_insert_with(CompilationCache::new);
        c.expander.core_env.clone()
    })
}

/// Register a REPL binding in the compilation cache.
///
/// After the REPL evaluates a `def`, the binding's value, signal, and
/// arity are added to PrimitiveMeta so subsequent compilations see it.
/// This is the same mechanism as `update_cache_with_stdlib` but for
/// individual bindings.
pub fn register_repl_binding(
    sym_id: crate::value::SymbolId,
    value: crate::value::Value,
    signal: crate::signals::Signal,
    arity: Option<crate::value::types::Arity>,
) {
    COMPILATION_CACHE.with(|cache| {
        let mut cache_ref = cache.borrow_mut();
        if let Some(c) = cache_ref.as_mut() {
            c.meta.signals.insert(sym_id, signal);
            c.meta.functions.insert(sym_id, value);
            if let Some(a) = arity {
                c.meta.arities.insert(sym_id, a);
            }
        }
    });
}

/// Merge macro definitions into the cached Expander.
///
/// Called by the REPL after compiling a form that contains `defmacro`.
/// The new macros become visible to all subsequent compilations.
pub fn register_repl_macros(macros: &std::collections::HashMap<String, crate::syntax::MacroDef>) {
    COMPILATION_CACHE.with(|cache| {
        let mut cache_ref = cache.borrow_mut();
        if let Some(c) = cache_ref.as_mut() {
            c.expander.merge_macros(macros);
        }
    });
}

/// Add stdlib exports to the cached PrimitiveMeta.
///
/// Called by `init_stdlib` after compiling and executing stdlib.lisp.
/// Each export is added to `meta.signals` and `meta.functions` so that
/// `bind_primitives` will pre-bind them for all subsequent compilations.
pub fn update_cache_with_stdlib(
    exports: std::collections::HashMap<
        crate::value::SymbolId,
        (crate::value::Value, crate::signals::Signal),
    >,
) {
    COMPILATION_CACHE.with(|cache| {
        let mut cache_ref = cache.borrow_mut();
        let c = cache_ref.get_or_insert_with(CompilationCache::new);
        for (sym_id, (value, signal)) in &exports {
            c.meta.signals.insert(*sym_id, *signal);
            c.meta.functions.insert(*sym_id, *value);
        }
    });
}

/// Look up or compute the signal projection for a file.
///
/// If the file has already been compiled and its projection cached, returns
/// the cached result. Otherwise, compiles the file (via `compile_file`),
/// caches the projection from the resulting bytecode, and returns it.
///
/// Returns `None` if the file's return value is not a projectable struct.
pub fn get_or_compile_projection(resolved_path: &str) -> Option<HashMap<String, Signal>> {
    // Check cache first (outside the compilation cache borrow)
    let cached = COMPILATION_CACHE.with(|cache| {
        cache
            .borrow()
            .as_ref()
            .and_then(|c| c.projections.get(resolved_path).cloned())
    });
    if let Some(proj) = cached {
        return proj;
    }

    // Read the file and compile it
    let source = std::fs::read_to_string(resolved_path).ok()?;
    let mut symbols = SymbolTable::new();
    let result = super::compile::compile_file(&source, &mut symbols, resolved_path).ok()?;
    let projection = result.bytecode.signal_projection;

    // Cache the result (even if None, to avoid re-compiling)
    COMPILATION_CACHE.with(|cache| {
        let mut cache_ref = cache.borrow_mut();
        if let Some(c) = cache_ref.as_mut() {
            c.projections
                .insert(resolved_path.to_string(), projection.clone());
        }
    });

    projection
}

/// Look up or compute the escape projection for a file.
///
/// Returns a map from field name to `true` (all closure fields are
/// rotation-safe and param-safe) for module-pattern files that return
/// a struct of closures.
pub fn get_or_compile_escape_projection(
    resolved_path: &str,
) -> Option<HashMap<String, crate::compiler::bytecode::FieldEscapeInfo>> {
    let cached = ESCAPE_PROJECTION_CACHE.with(|pc| pc.borrow().get(resolved_path).cloned());
    if let Some(proj) = cached {
        return proj;
    }

    // Read the file and compile it
    let source = std::fs::read_to_string(resolved_path).ok()?;
    let mut symbols = SymbolTable::new();
    let result = super::compile::compile_file(&source, &mut symbols, resolved_path).ok()?;
    let escape_proj = result.bytecode.escape_projection;

    // Cache signal projection too if not already cached
    let signal_proj = result.bytecode.signal_projection;
    PROJECTION_CACHE.with(|pc| {
        pc.borrow_mut()
            .entry(resolved_path.to_string())
            .or_insert(signal_proj);
    });

    ESCAPE_PROJECTION_CACHE.with(|pc| {
        pc.borrow_mut()
            .insert(resolved_path.to_string(), escape_proj.clone());
    });

    escape_proj
}
