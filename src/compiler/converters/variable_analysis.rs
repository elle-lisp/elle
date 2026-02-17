use super::super::ast::{Expr, Pattern};
use super::ScopeEntry;
use crate::value_old::SymbolId;

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
    for val in body_vals {
        if let Ok(inner_list) = val.list_to_vec() {
            if inner_list.is_empty() {
                continue;
            }
            if let Some(sym) = inner_list[0].as_symbol() {
                if let Some(name) = symbols.name(SymbolId(sym)) {
                    match name {
                        "define" => {
                            if inner_list.len() == 3 {
                                if let Some(def_name) = inner_list[1].as_symbol() {
                                    let def_name = SymbolId(def_name);
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
        Expr::Var(varref) => {
            use crate::binding::VarRef;
            // Adjust VarRef indices based on the closure environment layout.
            // The closure environment is [captures..., parameters..., locals...]
            match varref {
                VarRef::Upvalue { sym, index, .. } => {
                    // Upvalues need to be mapped to their position in the captures array.
                    // Look up the symbol in the capture map to get the correct index.
                    if let Some(&cap_pos) = capture_map.get(sym) {
                        *index = cap_pos;
                    }
                    // If not found in captures, it might be a global that was incorrectly
                    // marked as upvalue - leave the index as-is
                }
                VarRef::Local { index } => {
                    // Local variables in the current lambda need to be adjusted.
                    // The index was set during parsing based on the scope stack position.
                    // We need to map it to the closure environment layout:
                    // [captures..., parameters..., locals...]
                    //
                    // During parsing, local_index was the position in the scope's symbol list.
                    // We need to determine if this is a parameter or a locally-defined variable.
                    //
                    // For now, we assume the index is already correct for parameters
                    // (they're at positions 0..params.len()-1 in the scope).
                    // We adjust by adding captures.len() to account for the closure layout.
                    *index += captures.len();
                }
                VarRef::LetBound { sym } => {
                    // If this let-bound variable is captured, convert to Upvalue
                    if let Some(&cap_pos) = capture_map.get(sym) {
                        *varref = VarRef::Upvalue {
                            sym: *sym,
                            index: cap_pos,
                            is_param: false,
                        };
                    }
                    // Otherwise leave as LetBound for runtime lookup
                }
                VarRef::Global { .. } => {
                    // Globals don't need adjustment
                }
            }
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
        Expr::Lambda {
            captures: nested_captures,
            ..
        } => {
            // Adjust upvalue indices in nested lambda captures to match the enclosing
            // closure's environment layout. The captures were created with indices from
            // the enclosing scope's symbol list, but they need to be adjusted to match
            // the enclosing closure's environment layout: [captures..., parameters..., locals...]
            for capture_info in nested_captures.iter_mut() {
                if let crate::binding::VarRef::Upvalue { sym, index, .. } = &mut capture_info.source
                {
                    // Look up this symbol in the enclosing closure's environment
                    // The enclosing closure's environment layout is [captures..., parameters..., locals...]

                    // First, check if it's in the captures list
                    if let Some(&cap_pos) = capture_map.get(sym) {
                        *index = cap_pos;
                    } else if let Some(param_pos) = params.iter().position(|p| p == sym) {
                        // It's a parameter of the enclosing closure
                        *index = captures.len() + param_pos;
                    } else if let Some(local_pos) = locals.iter().position(|l| l == sym) {
                        // It's a locally-defined variable of the enclosing closure
                        *index = captures.len() + params.len() + local_pos;
                    }
                    // If not found in any of these, leave the index as-is (it might be a global)
                }
            }

            // DO NOT recursively adjust the nested lambda's body here!
            // The nested lambda's body was already adjusted when the nested lambda was parsed.
            // Adjusting it again would cause indices to be incremented multiple times.
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
        Expr::Set { target, value } => {
            use crate::binding::VarRef;
            // Adjust the target VarRef just like we do for Expr::Var
            match target {
                VarRef::Upvalue { sym, index, .. } => {
                    if let Some(&cap_pos) = capture_map.get(sym) {
                        *index = cap_pos;
                    }
                }
                VarRef::Local { index } => {
                    // Local variables need to be adjusted by adding captures.len()
                    *index += captures.len();
                }
                VarRef::LetBound { sym } => {
                    // If this let-bound variable is captured, convert to Upvalue
                    if let Some(&cap_pos) = capture_map.get(sym) {
                        *target = VarRef::Upvalue {
                            sym: *sym,
                            index: cap_pos,
                            is_param: false,
                        };
                    }
                }
                VarRef::Global { .. } => {
                    // Globals don't need adjustment
                }
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
        Expr::Yield(value) => {
            adjust_var_indices(value, captures, params, locals);
        }
        // Literals, GlobalVar, etc. don't need adjustment
        _ => {}
    }
}
