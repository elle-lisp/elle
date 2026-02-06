use super::ast::Expr;
use crate::value::SymbolId;
use std::collections::HashSet;

/// Analyze which variables from a given set are actually used in an expression
/// This is used to eliminate dead captures in closures
/// Returns the subset of candidates that are referenced in the expression
pub fn analyze_capture_usage(
    expr: &Expr,
    local_bindings: &HashSet<SymbolId>,
    candidates: &HashSet<SymbolId>,
) -> HashSet<SymbolId> {
    let mut used_vars = HashSet::new();

    match expr {
        Expr::Literal(_) => {
            // No variables in literals
        }

        Expr::Var(sym, _, _) => {
            // Variable reference - include if it's in candidates and not locally bound
            if candidates.contains(sym) && !local_bindings.contains(sym) {
                used_vars.insert(*sym);
            }
        }

        Expr::GlobalVar(_sym) => {
            // Global variables don't need to be captured
        }

        Expr::If { cond, then, else_ } => {
            used_vars.extend(analyze_capture_usage(cond, local_bindings, candidates));
            used_vars.extend(analyze_capture_usage(then, local_bindings, candidates));
            used_vars.extend(analyze_capture_usage(else_, local_bindings, candidates));
        }

        Expr::Begin(exprs) => {
            for e in exprs {
                used_vars.extend(analyze_capture_usage(e, local_bindings, candidates));
            }
        }

        Expr::Call { func, args, .. } => {
            used_vars.extend(analyze_capture_usage(func, local_bindings, candidates));
            for arg in args {
                used_vars.extend(analyze_capture_usage(arg, local_bindings, candidates));
            }
        }

        Expr::Lambda { params, body, .. } => {
            // Phase 4: Recurse into nested lambdas
            // Create new local bindings that include lambda parameters
            let mut new_bindings = local_bindings.clone();
            for param in params {
                new_bindings.insert(*param);
            }
            // Recurse into nested lambda body with extended bindings
            used_vars.extend(analyze_capture_usage(body, &new_bindings, candidates));
        }

        Expr::Let { bindings, body } => {
            // First, variables in the binding expressions can reference outer scope
            for (_, expr) in bindings {
                used_vars.extend(analyze_capture_usage(expr, local_bindings, candidates));
            }
            // Then, body can reference let-bound variables
            let mut new_bindings = local_bindings.clone();
            for (name, _) in bindings {
                new_bindings.insert(*name);
            }
            used_vars.extend(analyze_capture_usage(body, &new_bindings, candidates));
        }

        Expr::Set { var, value, .. } => {
            if candidates.contains(var) && !local_bindings.contains(var) {
                used_vars.insert(*var);
            }
            used_vars.extend(analyze_capture_usage(value, local_bindings, candidates));
        }

        Expr::Define { name: _, value } => {
            used_vars.extend(analyze_capture_usage(value, local_bindings, candidates));
        }

        Expr::While { cond, body } => {
            used_vars.extend(analyze_capture_usage(cond, local_bindings, candidates));
            used_vars.extend(analyze_capture_usage(body, local_bindings, candidates));
        }

        Expr::For { var, iter, body } => {
            used_vars.extend(analyze_capture_usage(iter, local_bindings, candidates));
            // Body can reference the loop variable
            let mut new_bindings = local_bindings.clone();
            new_bindings.insert(*var);
            used_vars.extend(analyze_capture_usage(body, &new_bindings, candidates));
        }

        Expr::Match {
            value,
            patterns,
            default,
        } => {
            used_vars.extend(analyze_capture_usage(value, local_bindings, candidates));
            for (_, expr) in patterns {
                used_vars.extend(analyze_capture_usage(expr, local_bindings, candidates));
            }
            if let Some(default_expr) = default {
                used_vars.extend(analyze_capture_usage(
                    default_expr,
                    local_bindings,
                    candidates,
                ));
            }
        }

        Expr::Try {
            body,
            catch,
            finally,
        } => {
            used_vars.extend(analyze_capture_usage(body, local_bindings, candidates));
            if let Some((var, handler)) = catch {
                let mut new_bindings = local_bindings.clone();
                new_bindings.insert(*var);
                used_vars.extend(analyze_capture_usage(handler, &new_bindings, candidates));
            }
            if let Some(finally_expr) = finally {
                used_vars.extend(analyze_capture_usage(
                    finally_expr,
                    local_bindings,
                    candidates,
                ));
            }
        }

        Expr::Block(exprs) => {
            for e in exprs {
                used_vars.extend(analyze_capture_usage(e, local_bindings, candidates));
            }
        }

        Expr::Letrec { bindings, body } => {
            for (_, expr) in bindings {
                used_vars.extend(analyze_capture_usage(expr, local_bindings, candidates));
            }
            used_vars.extend(analyze_capture_usage(body, local_bindings, candidates));
        }

        Expr::Cond { clauses, else_body } => {
            for (test, body) in clauses {
                used_vars.extend(analyze_capture_usage(test, local_bindings, candidates));
                used_vars.extend(analyze_capture_usage(body, local_bindings, candidates));
            }
            if let Some(else_expr) = else_body {
                used_vars.extend(analyze_capture_usage(else_expr, local_bindings, candidates));
            }
        }

        Expr::And(exprs) | Expr::Or(exprs) => {
            for e in exprs {
                used_vars.extend(analyze_capture_usage(e, local_bindings, candidates));
            }
        }

        _ => {
            // Other expression types don't affect usage
        }
    }

    used_vars
}

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
