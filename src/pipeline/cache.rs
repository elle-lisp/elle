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

/// Get or initialize the compilation cache, returning a VM with reset fiber,
/// a cloned Expander, and cloned PrimitiveMeta.
///
/// The VM's fiber is always reset before use. The Expander is cloned so that
/// each pipeline call gets independent expansion state (scope IDs, depth).
///
/// # Safety / Lifetime
///
/// Returns a raw pointer to the cached VM. The VM lives in a thread-local
/// RefCell; the borrow is released before this function returns, so the
/// caller must not call `get_compilation_cache` again while the VM pointer
/// is live. This is safe because pipeline functions are not re-entrant
/// (`eval_syntax` receives its own `&mut VM`, not the cache VM).
pub(super) fn get_compilation_cache() -> (*mut VM, Expander, PrimitiveMeta) {
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
        let vm_ptr = &mut c.vm as *mut VM;
        (vm_ptr, expander, meta)
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
