use super::super::ast::{Expr, Pattern};
use super::ScopeEntry;
use crate::value::SymbolId;

/// Extract all variable bindings from a pattern
pub fn extract_pattern_variables(pattern: &Pattern) -> Vec<SymbolId> {
    let mut vars = Vec::new();
    match pattern {
        Pattern::Var(sym_id) => {
            vars.push(*sym_id);
        }
        Pattern::Wildcard | Pattern::Literal(_) | Pattern::Nil => {
            // These don't bind variables
        }
        Pattern::List(patterns) => {
            // Extract variables from all list elements
            for p in patterns {
                vars.extend(extract_pattern_variables(p));
            }
        }
        Pattern::Cons { head, tail } => {
            // Extract from head and tail
            vars.extend(extract_pattern_variables(head));
            vars.extend(extract_pattern_variables(tail));
        }
        Pattern::Guard { pattern: inner, .. } => {
            // Extract from inner pattern
            vars.extend(extract_pattern_variables(inner));
        }
    }
    vars
}

/// Pre-scan body values for define names and register them in the current scope.
/// This enables mutual recursion and self-recursion by making all locally-defined
/// names visible before any lambda values are parsed.
pub fn pre_register_defines(
    body_vals: &[crate::value::Value],
    symbols: &crate::symbol::SymbolTable,
    scope_stack: &mut Vec<ScopeEntry>,
) {
    use crate::value::Value;

    for val in body_vals {
        if let Ok(inner_list) = val.list_to_vec() {
            if inner_list.is_empty() {
                continue;
            }
            if let Value::Symbol(sym) = &inner_list[0] {
                if let Some(name) = symbols.name(*sym) {
                    match name {
                        "define" => {
                            if inner_list.len() == 3 {
                                if let Ok(def_name) = inner_list[1].as_symbol() {
                                    if let Some(scope_entry) = scope_stack.last_mut() {
                                        if !scope_entry.symbols.contains(&def_name) {
                                            scope_entry.symbols.push(def_name);
                                        }
                                    }
                                }
                            }
                        }
                        "begin" => {
                            pre_register_defines(&inner_list[1..], symbols, scope_stack);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Adjust variable indices in an expression to account for the closure environment layout.
/// The closure environment is laid out as [captures..., parameters...]
///
/// During parsing, Var nodes contain indices relative to the scope_stack at parse time.
/// After computing captures, we need to convert these to absolute indices in the final environment.
///
/// For each Var:
/// - If it's a parameter of the current lambda: map to captures.len() + position_in_params
/// - If it's a captured value: map to position_in_captures_list
pub fn adjust_var_indices(
    expr: &mut Expr,
    captures: &[(SymbolId, usize, usize)],
    params: &[SymbolId],
    locals: &[SymbolId],
) {
    // Build a map of captures to their position in the list
    let mut capture_map: std::collections::HashMap<SymbolId, usize> =
        std::collections::HashMap::new();
    for (i, (sym, _, _)) in captures.iter().enumerate() {
        capture_map.insert(*sym, i);
    }

    // Build a map of parameters to their position in the list
    let mut param_map: std::collections::HashMap<SymbolId, usize> =
        std::collections::HashMap::new();
    for (i, sym) in params.iter().enumerate() {
        param_map.insert(*sym, i);
    }

    // Build a map of locally-defined variables to their position in the list
    let mut locals_map: std::collections::HashMap<SymbolId, usize> =
        std::collections::HashMap::new();
    for (i, sym) in locals.iter().enumerate() {
        locals_map.insert(*sym, i);
    }

    match expr {
        Expr::Var(sym_id, _depth, index) => {
            // Variables are adjusted based on their scope:
            // 1. Captures: map to position in captures list (0..captures.len()-1)
            // 2. Parameters: map to captures.len() + position
            // 3. Locals: map to captures.len() + params.len() + position
            if let Some(cap_pos) = capture_map.get(sym_id) {
                // This variable is a capture - map to its position in the captures list
                *index = *cap_pos;
            } else if let Some(param_pos) = param_map.get(sym_id) {
                // This variable is a parameter - map to captures.len() + position
                *index = captures.len() + param_pos;
            } else if let Some(local_pos) = locals_map.get(sym_id) {
                // This variable is defined locally within lambda body
                // Map to captures.len() + params.len() + position
                *index = captures.len() + params.len() + local_pos;
            }
            // If not found in any map, it's likely a global or error - leave as is
        }
        Expr::If { cond, then, else_ } => {
            adjust_var_indices(cond, captures, params, locals);
            adjust_var_indices(then, captures, params, locals);
            adjust_var_indices(else_, captures, params, locals);
        }
        Expr::Cond { clauses, else_body } => {
            for (test, body) in clauses {
                adjust_var_indices(test, captures, params, locals);
                adjust_var_indices(body, captures, params, locals);
            }
            if let Some(else_expr) = else_body {
                adjust_var_indices(else_expr, captures, params, locals);
            }
        }
        Expr::Begin(exprs) => {
            for e in exprs {
                adjust_var_indices(e, captures, params, locals);
            }
        }
        Expr::Block(exprs) => {
            for e in exprs {
                adjust_var_indices(e, captures, params, locals);
            }
        }
        Expr::Call { func, args, .. } => {
            adjust_var_indices(func, captures, params, locals);
            for arg in args {
                adjust_var_indices(arg, captures, params, locals);
            }
        }
        Expr::Lambda { .. } => {
            // Don't adjust nested lambda bodies at all - they have already been fully processed
            // with their own capture scopes, parameters, and indices when they were parsed.
            // Recursing into them would incorrectly adjust already-correct indices.
        }
        Expr::Let { bindings, body } => {
            for (_, expr) in bindings {
                adjust_var_indices(expr, captures, params, locals);
            }
            adjust_var_indices(body, captures, params, locals);
        }
        Expr::Letrec { bindings, body } => {
            for (_, expr) in bindings {
                adjust_var_indices(expr, captures, params, locals);
            }
            adjust_var_indices(body, captures, params, locals);
        }
        Expr::Set {
            var,
            depth: _,
            index,
            value,
        } => {
            // Remap the target variable index, same as Expr::Var
            if let Some(cap_pos) = capture_map.get(var) {
                *index = *cap_pos;
            } else if let Some(param_pos) = param_map.get(var) {
                *index = captures.len() + param_pos;
            } else if let Some(local_pos) = locals_map.get(var) {
                *index = captures.len() + params.len() + local_pos;
            }
            adjust_var_indices(value, captures, params, locals);
        }
        Expr::While { cond, body } => {
            adjust_var_indices(cond, captures, params, locals);
            adjust_var_indices(body, captures, params, locals);
        }
        Expr::For { iter, body, .. } => {
            adjust_var_indices(iter, captures, params, locals);
            adjust_var_indices(body, captures, params, locals);
        }
        Expr::Match {
            value,
            patterns,
            default,
        } => {
            adjust_var_indices(value, captures, params, locals);
            for (_, expr) in patterns {
                adjust_var_indices(expr, captures, params, locals);
            }
            if let Some(default_expr) = default {
                adjust_var_indices(default_expr, captures, params, locals);
            }
        }
        Expr::Try {
            body,
            catch,
            finally,
        } => {
            adjust_var_indices(body, captures, params, locals);
            if let Some((_, handler)) = catch {
                adjust_var_indices(handler, captures, params, locals);
            }
            if let Some(finally_expr) = finally {
                adjust_var_indices(finally_expr, captures, params, locals);
            }
        }
        Expr::And(exprs) | Expr::Or(exprs) => {
            for e in exprs {
                adjust_var_indices(e, captures, params, locals);
            }
        }
        Expr::Define { value, .. } => {
            adjust_var_indices(value, captures, params, locals);
        }
        Expr::DefMacro { body, .. } => {
            adjust_var_indices(body, captures, params, locals);
        }
        // Literals, GlobalVar, etc. don't need adjustment
        _ => {}
    }
}
