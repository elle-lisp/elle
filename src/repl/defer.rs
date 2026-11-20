// audited: 2026-11-19
//! Holding a prompt form whose references do not resolve yet, and retrying it
//! once later lines arrive.
//!
//! docs/bindings.md
//! docs/impl/region/rules.md
//!
//! Each line compiles alone, and a name exists from its def onward, so a
//! forward reference is an undefined variable. A `def` that fails that way is
//! kept and retried alone after each later line. Two kept defs that read each
//! other never resolve, because each retry still misses the other.

use crate::pipeline::{compile_file_repl, CompileCtx};
use crate::symbol::SymbolTable;
use crate::value::arena::{release_program_value, RootRef};
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

/// Recompile each deferred form alone. A form whose missing references are
/// now in the compilation cache compiles and is registered. Repeat until a
/// pass resolves nothing, because one resolved form may unblock another.
pub(super) fn try_resolve_deferred(
    deferred: &mut Vec<DeferredForm>,
    vm: &mut VM,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
) {
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
    let Ok(value) = vm.execute_scheduled(&result, cctx) else {
        return false;
    };
    let sym_id = symbols.intern(&form.name);
    let (signal, arity) = extract_signal_arity(&value);
    cctx.register_repl_binding(
        unsafe { &mut *vm.heap_ptr },
        sym_id,
        value,
        RootRef::Mint,
        signal,
        arity,
    );
    // The retry answers with the value like any other run, and nothing prints
    // it, so its owning reference goes back here (docs/impl/region/rules.md).
    release_program_value(unsafe { &mut *vm.heap_ptr }, value);
    true
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
