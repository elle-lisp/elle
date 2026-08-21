//! Linting rules for Elle code

use super::diagnostics::{Diagnostic, Severity};
use crate::primitives::registration::ALL_TABLES;
use crate::reader::SourceLoc;
use crate::value::types::Arity;
use crate::value::SymbolId;

/// Check arity of a function call
pub(crate) fn check_call_arity(
    func_sym: SymbolId,
    arg_count: usize,
    location: &Option<SourceLoc>,
    symbol_table: &crate::SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(func_name) = symbol_table.name(func_sym) {
        if let Some(arity) = builtin_arity(func_name) {
            if !arity.matches(arg_count) {
                let diag = Diagnostic::new(
                    Severity::Warning,
                    "W002",
                    "arity-mismatch",
                    format!(
                        "function '{}' expects {} argument(s) but got {}",
                        func_name, arity, arg_count
                    ),
                    location.clone(),
                );
                diagnostics.push(diag);
            }
        }
    }
}

/// Recommend an immutable binding for a mutable one that is never reassigned.
///
/// A binding declared mutable (`var`, or an `@`-prefixed `def`/`let` name) but
/// never the target of an `assign` is a *false-mutable*: its value may still be
/// mutated in place (e.g. `(let [buf @""] (push buf x))`), but the binding
/// itself never changes, so it can be a plain immutable `def`/`let`. The check
/// reads only the two arena facts that decide it — declared-immutability and
/// whether an `assign` ever targeted the binding — so it cannot confuse a
/// mutable binding with a mutable value.
///
/// Throwaway (`_`-prefixed), synthetic, primitive, and parameter bindings are
/// exempt. Callers invoke this at binding-introduction sites (`def`/`let`/
/// `letrec`); loop variables (rebound via `recur`, not `assign`) and pattern
/// bindings are excluded by never being passed here.
pub(crate) fn check_mutable_never_assigned(
    binding: crate::hir::Binding,
    arena: &crate::hir::BindingArena,
    location: &Option<SourceLoc>,
    symbol_table: &crate::SymbolTable,
    function: Option<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let inner = arena.get(binding);
    if inner.scope != crate::hir::arena::BindingScope::Local
        || inner.is_immutable
        || inner.is_mutated
        || inner.is_synthetic
        || inner.is_primitive
    {
        return;
    }
    let Some(name) = symbol_table.name(inner.name) else {
        return;
    };
    // `_`-prefixed names are the throwaway / compiler-temporary convention
    // (e.g. the `__destructure_tmp` binder); a lint that recommends making them
    // immutable is noise, not signal.
    if name.starts_with('_') {
        return;
    }
    let mut diag = Diagnostic::new(
        Severity::Warning,
        "W003",
        "mutable-binding-never-assigned",
        format!("mutable binding '{name}' is never reassigned"),
        location.clone(),
    );
    diag.suggestions.push(format!(
        "declare '{name}' immutable (use `def`/`let` without `@`, not `var`); \
         if its value is mutated in place, that is unaffected — only the binding changes"
    ));
    diag.function = function.map(str::to_string);
    diagnostics.push(diag);
}

/// Get arity of a built-in function by looking up `PrimitiveDef::PRIMITIVES` tables.
pub(crate) fn builtin_arity(name: &str) -> Option<Arity> {
    for table in ALL_TABLES {
        for def in *table {
            if def.name == name || def.aliases.contains(&name) {
                return Some(def.arity);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
