//! Linting rules for Elle code

use super::diagnostics::{
    Diagnostic, ARITY_MISMATCH, MUTABLE_BINDING_NEVER_ASSIGNED, NON_TAIL_SELF_RECURSION,
    UNUSED_BINDING,
};
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
                let diag = Diagnostic::warn(
                    ARITY_MISMATCH,
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

/// The name a lint rule may report for `binding`, or `None` when the binding is
/// not the user's to fix.
///
/// Three classes are silent for every binding rule. A `is_synthetic` binding is
/// a compiler temporary the user never wrote (`__destructure_tmp`, the
/// file-letrec statement wrappers). A `is_primitive` binding is injected into
/// every compilation unit by `bind_primitives`, so its presence says nothing
/// about the program. A `_`-prefixed name is the conventional "I know about
/// this one" marker. A binding whose symbol the table cannot name has nothing
/// to report.
fn reportable_name<'a>(
    inner: &crate::hir::BindingInner,
    symbol_table: &'a crate::SymbolTable,
) -> Option<&'a str> {
    if inner.is_synthetic || inner.is_primitive {
        return None;
    }
    let name = symbol_table.name(inner.name)?;
    if name.starts_with('_') {
        return None;
    }
    Some(name)
}

/// Warn when a binding is introduced and never read.
///
/// Zero uses is the shape shared by a misspelled reference (`reuslt` defined,
/// `result` read), by a definition whose last reader a refactoring removed, and
/// by an import nothing projects out of. The rule reads the def-use chains
/// `src/hir/defuse.rs` already builds — `used` holds every binding with at least
/// one use — so no binding carries a use counter of its own.
///
/// A use is any read: a `Var` node, or a closure capture. A self-recursive
/// function reads its own binding from its own body, so it counts as used.
/// Reassignment does not count: `assign` records a definition, so a binding that
/// is written and never read is still reported.
///
/// Callers invoke this at binding-introduction sites (`def`/`let`/`letrec`).
/// Lambda parameters, loop variables, and pattern bindings never reach it.
pub(crate) fn check_unused_binding(
    binding: crate::hir::Binding,
    arena: &crate::hir::BindingArena,
    used: &std::collections::HashSet<crate::hir::Binding>,
    location: &Option<SourceLoc>,
    symbol_table: &crate::SymbolTable,
    function: Option<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if used.contains(&binding) {
        return;
    }
    let Some(name) = reportable_name(arena.get(binding), symbol_table) else {
        return;
    };
    let mut diag = Diagnostic::warn(
        UNUSED_BINDING,
        format!("binding '{name}' is never used"),
        location.clone(),
    );
    diag.suggestions.push(format!(
        "remove '{name}', or rename it to '_{name}' if the binding is deliberately unused"
    ));
    diag.function = function.map(str::to_string);
    diagnostics.push(diag);
}

/// Warn when a function calls itself outside tail position.
///
/// Every such call keeps its caller's frame alive, so the recursion depth is
/// bounded by the stack and deep input halts the program. A tail self-call
/// replaces the frame and costs one frame at any depth, and the rewrite that
/// gets there — an accumulator parameter, or `each`/`while` — is cheap while the
/// code is being written.
///
/// Scope is direct self-recursion: the enclosing function's own binding stands
/// in the call's function position. Mutual recursion needs call-graph reasoning
/// and is not covered.
pub(crate) fn check_non_tail_self_recursion(
    enclosing: crate::hir::Binding,
    callee: crate::hir::Binding,
    is_tail: bool,
    arena: &crate::hir::BindingArena,
    location: &Option<SourceLoc>,
    symbol_table: &crate::SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if is_tail || callee != enclosing {
        return;
    }
    let Some(name) = reportable_name(arena.get(enclosing), symbol_table) else {
        return;
    };
    let mut diag = Diagnostic::warn(
        NON_TAIL_SELF_RECURSION,
        format!("'{name}' calls itself outside tail position, so the stack grows with the recursion depth"),
        location.clone(),
    );
    diag.suggestions.push(format!(
        "give '{name}' an accumulator parameter so the self-call is the whole result, \
         or restate the recursion as `each`/`while`"
    ));
    diag.function = Some(name.to_string());
    diagnostics.push(diag);
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
/// exempt ([`reportable_name`]). Callers invoke this at binding-introduction
/// sites (`def`/`let`/`letrec`); loop variables (rebound via `recur`, not
/// `assign`) and pattern bindings are excluded by never being passed here.
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
    {
        return;
    }
    let Some(name) = reportable_name(inner, symbol_table) else {
        return;
    };
    let mut diag = Diagnostic::warn(
        MUTABLE_BINDING_NEVER_ASSIGNED,
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
