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
            let mut growing_bindings = local_bindings.clone();
            for e in exprs {
                // Check if this is a define - if so, add it to local bindings for subsequent expressions
                if let Expr::Define { name, value } = e {
                    // First, analyze the value with current bindings
                    free_vars.extend(analyze_free_vars(value, &growing_bindings));
                    // Then add the defined name to bindings for subsequent expressions
                    growing_bindings.insert(*name);
                } else {
                    // For non-define expressions, use the growing set of bindings
                    free_vars.extend(analyze_free_vars(e, &growing_bindings));
                }
            }
        }

        Expr::Block(exprs) => {
            let mut growing_bindings = local_bindings.clone();
            for e in exprs {
                // Check if this is a define - if so, add it to local bindings for subsequent expressions
                if let Expr::Define { name, value } = e {
                    // First, analyze the value with current bindings
                    free_vars.extend(analyze_free_vars(value, &growing_bindings));
                    // Then add the defined name to bindings for subsequent expressions
                    growing_bindings.insert(*name);
                } else {
                    // For non-define expressions, use the growing set of bindings
                    free_vars.extend(analyze_free_vars(e, &growing_bindings));
                }
            }
        }

        Expr::Call { func, args, .. } => {
            free_vars.extend(analyze_free_vars(func, local_bindings));
            for arg in args {
                free_vars.extend(analyze_free_vars(arg, local_bindings));
            }
        }

        Expr::Lambda { params, body, .. } => {
            // For nested lambdas, analyze with only the lambda's own parameters as bindings.
            // This identifies variables that the nested lambda needs to capture.
            // However, we must filter the results: only propagate vars that are ALSO free
            // in the current scope. Variables that are locally defined in the current scope
            // (in local_bindings) are NOT free at this level — they're only free inside
            // the nested lambda (which will capture them).
            let mut new_bindings = HashSet::new();
            for param in params {
                new_bindings.insert(*param);
            }
            let inner_free = analyze_free_vars(body, &new_bindings);
            for var in inner_free {
                if !local_bindings.contains(&var) {
                    free_vars.insert(var);
                }
            }
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

/// Analyze which locally-defined variables in an expression are referenced by nested lambdas
/// Returns a set of SymbolIds that are defined locally and referenced in nested lambdas
/// These variables need cell boxing for shared mutable access
pub fn analyze_local_vars_captured_by_nested_lambdas(expr: &Expr) -> HashSet<SymbolId> {
    match expr {
        Expr::Begin(exprs) | Expr::Block(exprs) => analyze_local_captures_in_seq(exprs),
        Expr::Lambda { .. } => {
            // Don't look inside nested lambdas - they're analyzed separately
            // Each lambda is analyzed independently with its own scope
            HashSet::new()
        }
        Expr::If { cond, then, else_ } => {
            let mut result = analyze_local_vars_captured_by_nested_lambdas(cond);
            result.extend(analyze_local_vars_captured_by_nested_lambdas(then));
            result.extend(analyze_local_vars_captured_by_nested_lambdas(else_));
            result
        }
        Expr::Call { func, args, .. } => {
            let mut result = analyze_local_vars_captured_by_nested_lambdas(func);
            for arg in args {
                result.extend(analyze_local_vars_captured_by_nested_lambdas(arg));
            }
            result
        }
        Expr::Let { bindings, body } | Expr::Letrec { bindings, body } => {
            let mut result = HashSet::new();
            for (_, expr) in bindings {
                result.extend(analyze_local_vars_captured_by_nested_lambdas(expr));
            }
            result.extend(analyze_local_vars_captured_by_nested_lambdas(body));
            result
        }
        _ => HashSet::new(),
    }
}

/// Helper to find locally-defined variables that are captured by nested lambdas
fn analyze_local_captures_in_seq(exprs: &[Expr]) -> HashSet<SymbolId> {
    let mut captured_by_nested = HashSet::new();
    let mut locally_defined = HashSet::new();

    for e in exprs {
        // Track which variables are defined at this level
        if let Expr::Define { name, .. } = e {
            locally_defined.insert(*name);
        } else {
            // For each non-define expression, check if it's a lambda that captures locals
            if let Expr::Lambda { body, params, .. } = e {
                // Find free variables in the nested lambda's body
                let nested_local_bindings = params.iter().copied().collect::<HashSet<_>>();
                let free_in_nested = analyze_free_vars(body, &nested_local_bindings);
                // Variables that are locally defined and free in nested lambda need cells
                for var in free_in_nested {
                    if locally_defined.contains(&var) {
                        captured_by_nested.insert(var);
                    }
                }
            }
        }
        // Also recursively check for deeper nesting
        captured_by_nested.extend(analyze_local_vars_captured_by_nested_lambdas(e));
    }

    captured_by_nested
}

/// Analyze which variables are mutated with set! in an expression
/// This is used to determine which captured variables need cell boxing
pub fn analyze_mutated_vars(expr: &Expr) -> HashSet<SymbolId> {
    let mut mutated = HashSet::new();

    match expr {
        Expr::Set { var, .. } => {
            mutated.insert(*var);
        }

        Expr::If { cond, then, else_ } => {
            mutated.extend(analyze_mutated_vars(cond));
            mutated.extend(analyze_mutated_vars(then));
            mutated.extend(analyze_mutated_vars(else_));
        }

        Expr::Begin(exprs) | Expr::Block(exprs) => {
            for e in exprs {
                mutated.extend(analyze_mutated_vars(e));
            }
        }

        Expr::Call { func, args, .. } => {
            mutated.extend(analyze_mutated_vars(func));
            for arg in args {
                mutated.extend(analyze_mutated_vars(arg));
            }
        }

        Expr::Lambda { body: _, .. } => {
            // Don't collect mutations from nested lambdas - only the current level matters
            // Nested lambda mutations are separate from the outer lambda's concerns
        }

        Expr::Let { bindings, body } => {
            for (_, expr) in bindings {
                mutated.extend(analyze_mutated_vars(expr));
            }
            mutated.extend(analyze_mutated_vars(body));
        }

        Expr::Letrec { bindings, body } => {
            for (_, expr) in bindings {
                mutated.extend(analyze_mutated_vars(expr));
            }
            mutated.extend(analyze_mutated_vars(body));
        }

        Expr::Define { value, .. } => {
            mutated.extend(analyze_mutated_vars(value));
        }

        Expr::While { cond, body } => {
            mutated.extend(analyze_mutated_vars(cond));
            mutated.extend(analyze_mutated_vars(body));
        }

        Expr::For { iter, body, .. } => {
            mutated.extend(analyze_mutated_vars(iter));
            mutated.extend(analyze_mutated_vars(body));
        }

        Expr::Match {
            value,
            patterns,
            default,
        } => {
            mutated.extend(analyze_mutated_vars(value));
            for (_, expr) in patterns {
                mutated.extend(analyze_mutated_vars(expr));
            }
            if let Some(default_expr) = default {
                mutated.extend(analyze_mutated_vars(default_expr));
            }
        }

        Expr::Try {
            body,
            catch,
            finally,
        } => {
            mutated.extend(analyze_mutated_vars(body));
            if let Some((_, handler)) = catch {
                mutated.extend(analyze_mutated_vars(handler));
            }
            if let Some(finally_expr) = finally {
                mutated.extend(analyze_mutated_vars(finally_expr));
            }
        }

        Expr::Cond { clauses, else_body } => {
            for (test, body) in clauses {
                mutated.extend(analyze_mutated_vars(test));
                mutated.extend(analyze_mutated_vars(body));
            }
            if let Some(else_expr) = else_body {
                mutated.extend(analyze_mutated_vars(else_expr));
            }
        }

        _ => {
            // Other expression types don't mutate variables
        }
    }

    mutated
}
