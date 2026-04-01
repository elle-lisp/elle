//! Compilation cache: thread-local VM, Expander, and PrimitiveMeta.

use crate::primitives::def::PrimitiveMeta;
use crate::primitives::register_primitives;
use crate::symbol::SymbolTable;
use crate::syntax::Expander;
use crate::vm::VM;

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
}

thread_local! {
    static COMPILATION_CACHE: std::cell::RefCell<Option<CompilationCache>> =
        const { std::cell::RefCell::new(None) };
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
        let c = cache_ref.get_or_insert_with(|| {
            let mut vm = VM::new();
            let mut init_symbols = SymbolTable::new();
            let meta = register_primitives(&mut vm, &mut init_symbols);
            let mut expander = Expander::new();
            // Prelude loading needs the VM for macro body evaluation.
            // The init_symbols is throwaway — prelude is 100% defmacro,
            // so handle_defmacro doesn't touch SymbolTable.
            expander
                .load_prelude(&mut init_symbols, &mut vm)
                .expect("prelude loading must succeed");
            CompilationCache { vm, expander, meta }
        });

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
        let c = cache_ref.get_or_insert_with(|| {
            let mut vm = VM::new();
            let mut init_symbols = SymbolTable::new();
            let meta = register_primitives(&mut vm, &mut init_symbols);
            let mut expander = Expander::new();
            expander
                .load_prelude(&mut init_symbols, &mut vm)
                .expect("prelude loading must succeed");
            CompilationCache { vm, expander, meta }
        });
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
        let c = cache_ref.get_or_insert_with(|| {
            let mut vm = VM::new();
            let mut init_symbols = SymbolTable::new();
            let meta = register_primitives(&mut vm, &mut init_symbols);
            let mut expander = Expander::new();
            expander
                .load_prelude(&mut init_symbols, &mut vm)
                .expect("prelude loading must succeed");
            CompilationCache { vm, expander, meta }
        });
        for (sym_id, (value, signal)) in &exports {
            c.meta.signals.insert(*sym_id, *signal);
            c.meta.functions.insert(*sym_id, *value);
        }
    });
}
