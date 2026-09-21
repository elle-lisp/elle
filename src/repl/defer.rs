// audited: 2026-09-20
//! Holding a prompt form whose references do not resolve yet, and retrying it
//! once later lines arrive.
//!
//! docs/pipeline.md
//!
//! A file compiles as one letrec, so a function may call one defined below it.
//! A session has no such file: each line compiles alone, and a forward
//! reference is an undefined variable. A `def` that fails that way is kept and
//! retried, alone first; a group that only compiles together is retried as one
//! letrec, which is what lets two prompt functions call each other.

use crate::pipeline::{compile_file_repl, CompileCtx};
use crate::symbol::SymbolTable;
use crate::value::arena::RootRef;
use crate::vm::VM;

use super::eval::extract_signal_arity;
use super::read::FormInfo;

/// A def/defn form whose compilation was deferred due to undefined
/// variable references. Retried after subsequent definitions arrive.
pub(super) struct DeferredForm {
    source: String,
    pub(super) name: String,
    pub(super) missing: Vec<String>,
}

/// Check whether a compilation error is a deferrable undefined-variable
/// error on a def/defn form. Only simple (non-destructuring) defs are
/// deferred.
pub(super) fn try_defer(form: &FormInfo, error: &str) -> Option<DeferredForm> {
    if form.bindings.len() != 1 {
        return None;
    }
    if !error.contains("undefined variable:") {
        return None;
    }
    let undefined_vars = extract_undefined_vars(error);
    if undefined_vars.is_empty() {
        return None;
    }
    Some(DeferredForm {
        source: form.source.clone(),
        name: form.bindings[0].name.clone(),
        missing: undefined_vars,
    })
}

/// Extract undefined variable names from a compilation error message.
fn extract_undefined_vars(error: &str) -> Vec<String> {
    let mut vars = Vec::new();
    for line in error.lines() {
        if let Some(idx) = line.find("undefined variable: ") {
            let rest = &line[idx + "undefined variable: ".len()..];
            let name: String = rest
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '(')
                .collect();
            if !name.is_empty() {
                vars.push(name);
            }
        }
    }
    vars
}

/// Try to resolve deferred forms. Two phases:
///
/// 1. **Individual**: recompile each deferred form alone. If its
///    missing references are now in the compilation cache, it
///    compiles and gets registered. Repeat until no progress.
///
/// 2. **Batch**: compile all remaining deferred forms together as a
///    single letrec. This handles mutual recursion: the letrec
///    pre-binds all names, allowing them to reference each other.
pub(super) fn try_resolve_deferred(
    deferred: &mut Vec<DeferredForm>,
    vm: &mut VM,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
) {
    if deferred.is_empty() {
        return;
    }

    // Phase 1: individual resolution (fixpoint loop)
    let mut changed = true;
    while changed {
        changed = false;
        let mut i = 0;
        while i < deferred.len() {
            if try_resolve_single(&deferred[i], vm, symbols, cctx) {
                eprintln!("{}: resolved", deferred[i].name);
                deferred.remove(i);
                changed = true;
            } else {
                i += 1;
            }
        }
    }

    // Phase 2: batch resolution for mutual recursion
    if deferred.len() >= 2 && try_batch_resolve(deferred, vm, symbols, cctx) {
        // Batch resolved some forms; try individual again
        // (resolving a batch may unblock other deferred forms).
        try_resolve_deferred(deferred, vm, symbols, cctx);
    }
}

/// Try to compile and register a single deferred form.
fn try_resolve_single(
    form: &DeferredForm,
    vm: &mut VM,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
) -> bool {
    let Ok((result, expander)) = compile_file_repl(&form.source, symbols, cctx, "<repl>") else {
        return false;
    };
    cctx.register_repl_macros(expander.macros());
    let Ok(value) = vm.execute_scheduled(&result.bytecode, cctx) else {
        return false;
    };
    let sym_id = symbols.intern(&form.name);
    let (signal, arity) = extract_signal_arity(&value);
    cctx.register_repl_binding(
        unsafe { &mut *vm.heap_ptr },
        sym_id,
        value,
        RootRef::Take,
        signal,
        arity,
    );
    true
}

/// Batch-compile all deferred forms as a single letrec. The letrec
/// pre-binds every name, enabling mutual recursion among the group.
/// A trailing tuple expression extracts each binding's value.
fn try_batch_resolve(
    deferred: &mut Vec<DeferredForm>,
    vm: &mut VM,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
) -> bool {
    let mut combined = String::new();
    let mut all_names: Vec<String> = Vec::new();

    for form in deferred.iter() {
        combined.push_str(&form.source);
        combined.push('\n');
        all_names.push(form.name.clone());
    }

    // Trailing tuple: [name1 name2 ...]
    combined.push_str(&format!("[{}]", all_names.join(" ")));

    let Ok((result, expander)) = compile_file_repl(&combined, symbols, cctx, "<repl>") else {
        return false;
    };
    cctx.register_repl_macros(expander.macros());
    let Ok(tuple_val) = vm.execute_scheduled(&result.bytecode, cctx) else {
        return false;
    };

    if let Some(items) = tuple_val.as_array() {
        for (form, val) in deferred.iter().zip(items.iter()) {
            let sym_id = symbols.intern(&form.name);
            let (signal, arity) = extract_signal_arity(val);
            cctx.register_repl_binding(
                unsafe { &mut *vm.heap_ptr },
                sym_id,
                *val,
                RootRef::Take,
                signal,
                arity,
            );
        }
        let names: Vec<&str> = all_names.iter().map(|s| s.as_str()).collect();
        eprintln!("{}: resolved", names.join(", "));
        deferred.clear();
        true
    } else {
        false
    }
}

/// Report unresolved deferred forms at session end. Returns true if
/// any exist (indicating an error).
pub(super) fn report_unresolved(deferred: &[DeferredForm]) -> bool {
    for form in deferred {
        eprintln!(
            "✗ {}: unresolved ({} undefined)",
            form.name,
            form.missing.join(", ")
        );
    }
    !deferred.is_empty()
}
