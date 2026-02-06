use super::ast::Expr;
use crate::value::SymbolId;
use rustc_hash::FxHashMap;

/// Tracks the environment layout for a lambda during capture resolution.
/// The runtime environment is [captures..., params...], so we need to know
/// where each symbol lives in that flat array.
struct LambdaEnvInfo {
    num_captures: usize,
    symbol_to_env_index: FxHashMap<SymbolId, usize>,
}

/// Entry point: walk the AST and fix capture indices so that when a nested
/// lambda captures a variable from an outer lambda, the index accounts for
/// the source lambda's captures offset.
pub fn resolve_captures(expr: &mut Expr) {
    let mut env_stack: Vec<LambdaEnvInfo> = Vec::new();
    resolve_in_expr(expr, &mut env_stack);
}

fn resolve_in_expr(expr: &mut Expr, env_stack: &mut Vec<LambdaEnvInfo>) {
    match expr {
        Expr::Lambda {
            params,
            body,
            captures,
        } => {
            // Phase A: Fix capture indices.
            // For each capture (sym, depth, index) where index != usize::MAX (not global):
            // The index currently points to the parameter index in the source scope.
            // We need to offset it by the source scope's num_captures, because the
            // runtime env layout is [captures..., params...].
            for (_sym, depth, index) in captures.iter_mut() {
                if *index == usize::MAX {
                    continue; // Global variable, skip
                }
                if env_stack.is_empty() {
                    continue; // Top-level lambda, no outer lambda env to adjust against
                }
                // depth is the absolute scope depth from value_to_expr_with_scope.
                // depth=0 means the variable is in the outermost scope (bottom of scope_stack).
                // The env_stack mirrors the scope_stack for lambdas we've entered.
                // env_stack[depth] is the source lambda where the variable was found.
                if *depth < env_stack.len() {
                    *index += env_stack[*depth].num_captures;
                }
            }

            // Phase B: Build LambdaEnvInfo for this lambda.
            // Environment layout: [capture_0, capture_1, ..., param_0, param_1, ...]
            let mut symbol_to_env_index = FxHashMap::default();
            for (i, (sym, _, _)) in captures.iter().enumerate() {
                symbol_to_env_index.insert(*sym, i);
            }
            for (i, param) in params.iter().enumerate() {
                symbol_to_env_index.insert(*param, captures.len() + i);
            }
            let info = LambdaEnvInfo {
                num_captures: captures.len(),
                symbol_to_env_index,
            };

            // Phase C: Remap body vars that reference captures (depth > 0 vars in body).
            // These are variables that the body references from outer scopes — they should
            // now point to the capture slot in THIS lambda's environment.
            remap_body_vars(body, &info);

            // Phase D: Push this lambda's info, recurse into body, pop.
            env_stack.push(info);
            resolve_in_expr(body, env_stack);
            env_stack.pop();
        }

        // For all other compound Expr variants, recurse into children:
        Expr::If { cond, then, else_ } => {
            resolve_in_expr(cond, env_stack);
            resolve_in_expr(then, env_stack);
            resolve_in_expr(else_, env_stack);
        }
        Expr::Cond { clauses, else_body } => {
            for (test, body) in clauses {
                resolve_in_expr(test, env_stack);
                resolve_in_expr(body, env_stack);
            }
            if let Some(e) = else_body {
                resolve_in_expr(e, env_stack);
            }
        }
        Expr::Begin(exprs) => {
            for e in exprs {
                resolve_in_expr(e, env_stack);
            }
        }
        Expr::Call { func, args, .. } => {
            resolve_in_expr(func, env_stack);
            for a in args {
                resolve_in_expr(a, env_stack);
            }
        }
        Expr::Let { bindings, body } => {
            for (_, e) in bindings {
                resolve_in_expr(e, env_stack);
            }
            resolve_in_expr(body, env_stack);
        }
        Expr::Set { value, .. } => {
            resolve_in_expr(value, env_stack);
        }
        Expr::Define { value, .. } => {
            resolve_in_expr(value, env_stack);
        }
        Expr::While { cond, body } => {
            resolve_in_expr(cond, env_stack);
            resolve_in_expr(body, env_stack);
        }
        Expr::For { iter, body, .. } => {
            resolve_in_expr(iter, env_stack);
            resolve_in_expr(body, env_stack);
        }
        Expr::Match {
            value,
            patterns,
            default,
        } => {
            resolve_in_expr(value, env_stack);
            for (_, e) in patterns {
                resolve_in_expr(e, env_stack);
            }
            if let Some(d) = default {
                resolve_in_expr(d, env_stack);
            }
        }
        Expr::Try {
            body,
            catch,
            finally,
        } => {
            resolve_in_expr(body, env_stack);
            if let Some((_, handler)) = catch {
                resolve_in_expr(handler, env_stack);
            }
            if let Some(f) = finally {
                resolve_in_expr(f, env_stack);
            }
        }
        Expr::And(exprs) | Expr::Or(exprs) => {
            for e in exprs {
                resolve_in_expr(e, env_stack);
            }
        }
        Expr::DefMacro { body, .. } => {
            resolve_in_expr(body, env_stack);
        }
        Expr::Module { body, .. } => {
            resolve_in_expr(body, env_stack);
        }
        // Leaf nodes: no-op
        _ => {}
    }
}

/// Fix variable references in a lambda body that point to captured variables.
/// A Var(sym, depth, index) with depth > 0 means "variable from an outer scope" —
/// it should be remapped to point to the capture slot in the current lambda's env.
///
/// CRITICAL: Do NOT recurse into nested Lambda nodes. Each lambda handles its own
/// body remapping during its own resolve_in_expr call.
fn remap_body_vars(expr: &mut Expr, env_info: &LambdaEnvInfo) {
    match expr {
        Expr::Var(sym, depth, index) => {
            if *depth > 0 {
                // This variable references an outer scope. It should be in our captures.
                if let Some(&env_idx) = env_info.symbol_to_env_index.get(sym) {
                    *index = env_idx;
                }
            }
        }
        Expr::Set {
            var,
            depth,
            index,
            value,
        } => {
            if *depth > 0 {
                if let Some(&env_idx) = env_info.symbol_to_env_index.get(var) {
                    *index = env_idx;
                }
            }
            remap_body_vars(value, env_info);
        }
        // DO NOT recurse into nested lambdas
        Expr::Lambda { .. } => (),

        // Recurse into all other compound nodes:
        Expr::If { cond, then, else_ } => {
            remap_body_vars(cond, env_info);
            remap_body_vars(then, env_info);
            remap_body_vars(else_, env_info);
        }
        Expr::Cond { clauses, else_body } => {
            for (test, body) in clauses {
                remap_body_vars(test, env_info);
                remap_body_vars(body, env_info);
            }
            if let Some(e) = else_body {
                remap_body_vars(e, env_info);
            }
        }
        Expr::Begin(exprs) => {
            for e in exprs {
                remap_body_vars(e, env_info);
            }
        }
        Expr::Call { func, args, .. } => {
            remap_body_vars(func, env_info);
            for a in args {
                remap_body_vars(a, env_info);
            }
        }
        Expr::Let { bindings, body } => {
            for (_, e) in bindings {
                remap_body_vars(e, env_info);
            }
            remap_body_vars(body, env_info);
        }
        Expr::Define { value, .. } => {
            remap_body_vars(value, env_info);
        }
        Expr::While { cond, body } => {
            remap_body_vars(cond, env_info);
            remap_body_vars(body, env_info);
        }
        Expr::For { iter, body, .. } => {
            remap_body_vars(iter, env_info);
            remap_body_vars(body, env_info);
        }
        Expr::Match {
            value,
            patterns,
            default,
        } => {
            remap_body_vars(value, env_info);
            for (_, e) in patterns {
                remap_body_vars(e, env_info);
            }
            if let Some(d) = default {
                remap_body_vars(d, env_info);
            }
        }
        Expr::Try {
            body,
            catch,
            finally,
        } => {
            remap_body_vars(body, env_info);
            if let Some((_, handler)) = catch {
                remap_body_vars(handler, env_info);
            }
            if let Some(f) = finally {
                remap_body_vars(f, env_info);
            }
        }
        Expr::And(exprs) | Expr::Or(exprs) => {
            for e in exprs {
                remap_body_vars(e, env_info);
            }
        }
        Expr::DefMacro { body, .. } => {
            remap_body_vars(body, env_info);
        }
        Expr::Module { body, .. } => {
            remap_body_vars(body, env_info);
        }
        // Leaf nodes: no-op
        _ => {}
    }
}
