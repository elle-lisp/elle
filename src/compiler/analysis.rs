use super::ast::Expr;
use crate::value::SymbolId;
use std::collections::HashSet;

/// Analyze free variables in an expression
/// Returns the set of variable symbols that are referenced but not bound locally
pub fn analyze_free_vars(expr: &Expr, local_bindings: &HashSet<SymbolId>) -> HashSet<SymbolId> {
    let mut free_vars = HashSet::new();

    match expr {
        Expr::Literal(_) => {
            // No variables in literals
        }

        Expr::Var(sym, _, _) => {
            // Variable reference - include if not locally bound
            if !local_bindings.contains(sym) {
                free_vars.insert(*sym);
            }
        }

        Expr::GlobalVar(_sym) => {
            // Global variables don't need to be captured - they're accessed at runtime
            // This includes built-in functions like *, +, -, etc.
        }

        Expr::If { cond, then, else_ } => {
            free_vars.extend(analyze_free_vars(cond, local_bindings));
            free_vars.extend(analyze_free_vars(then, local_bindings));
            free_vars.extend(analyze_free_vars(else_, local_bindings));
        }

        Expr::Begin(exprs) => {
            for e in exprs {
                free_vars.extend(analyze_free_vars(e, local_bindings));
            }
        }

        Expr::Call { func, args, .. } => {
            free_vars.extend(analyze_free_vars(func, local_bindings));
            for arg in args {
                free_vars.extend(analyze_free_vars(arg, local_bindings));
            }
        }

        Expr::Lambda { params, body, .. } => {
            // Create new local bindings that include lambda parameters
            let mut new_bindings = local_bindings.clone();
            for param in params {
                new_bindings.insert(*param);
            }
            free_vars.extend(analyze_free_vars(body, &new_bindings));
        }

        Expr::Let { bindings, body } => {
            // First, variables in the binding expressions can reference outer scope
            for (_, expr) in bindings {
                free_vars.extend(analyze_free_vars(expr, local_bindings));
            }
            // Then, body can reference let-bound variables
            let mut new_bindings = local_bindings.clone();
            for (name, _) in bindings {
                new_bindings.insert(*name);
            }
            free_vars.extend(analyze_free_vars(body, &new_bindings));
        }

        Expr::Set { var, value, .. } => {
            if !local_bindings.contains(var) {
                free_vars.insert(*var);
            }
            free_vars.extend(analyze_free_vars(value, local_bindings));
        }

        Expr::Define { name: _, value } => {
            free_vars.extend(analyze_free_vars(value, local_bindings));
        }

        Expr::While { cond, body } => {
            free_vars.extend(analyze_free_vars(cond, local_bindings));
            free_vars.extend(analyze_free_vars(body, local_bindings));
        }

        Expr::For { var, iter, body } => {
            free_vars.extend(analyze_free_vars(iter, local_bindings));
            // Body can reference the loop variable
            let mut new_bindings = local_bindings.clone();
            new_bindings.insert(*var);
            free_vars.extend(analyze_free_vars(body, &new_bindings));
        }

        Expr::Match {
            value,
            patterns,
            default,
        } => {
            free_vars.extend(analyze_free_vars(value, local_bindings));
            for (_, expr) in patterns {
                free_vars.extend(analyze_free_vars(expr, local_bindings));
            }
            if let Some(default_expr) = default {
                free_vars.extend(analyze_free_vars(default_expr, local_bindings));
            }
        }

        Expr::Try {
            body,
            catch,
            finally,
        } => {
            free_vars.extend(analyze_free_vars(body, local_bindings));
            if let Some((var, handler)) = catch {
                let mut new_bindings = local_bindings.clone();
                new_bindings.insert(*var);
                free_vars.extend(analyze_free_vars(handler, &new_bindings));
            }
            if let Some(finally_expr) = finally {
                free_vars.extend(analyze_free_vars(finally_expr, local_bindings));
            }
        }

        _ => {
            // Other expression types (Quote, Quasiquote, Module, etc.) don't affect free vars
        }
    }

    free_vars
}
