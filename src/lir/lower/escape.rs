//! Escape analysis for scope allocation.
//!
//! Determines whether a `let`/`letrec`/`block` scope's allocations can be
//! safely released at scope exit (via `RegionEnter`/`RegionExit`).
//!
//! **Error asymmetry:** A false positive (scope-allocating something that
//! escapes) is use-after-free. A false negative (not scope-allocating
//! something safe) is the status quo. Every function here errs toward
//! returning `false` (conservative).
//!
//! ## Safety conditions
//!
//! A scope is safe to allocate when ALL conditions hold:
//!
//! 1. No binding is captured by a nested lambda (`is_captured()`)
//! 2. Body cannot suspend (`may_suspend()`)
//! 3. Body result is provably a NaN-boxed immediate (`result_is_safe`)
//! 4. Body contains no `set` to bindings outside this scope
//!    (`body_contains_outward_set`)
//! 5. Body contains no `break` (`hir_contains_break`) — a break carries
//!    a value past the compensating `RegionExit`
//!
//! ## What `RegionExit` frees
//!
//! `RegionExit` runs destructors for ALL heap objects allocated between
//! `RegionEnter` and `RegionExit` — including objects the body allocated
//! (not just binding values). This is why condition 3 is required: the
//! body's result, if heap-allocated inside the scope, gets freed before
//! the caller uses it.

use super::Lowerer;
use crate::hir::{Binding, CallArg, Hir, HirKind};
use crate::lir::intrinsics::IntrinsicOp;

impl Lowerer {
    /// Check if the result of a HIR expression is provably a NaN-boxed
    /// immediate (not a heap pointer to something allocated inside the scope).
    ///
    /// Returns `true` only for expressions that are guaranteed to produce
    /// non-heap values: literals, intrinsic arithmetic/comparison/logical
    /// operations, and control flow where all result positions are safe.
    ///
    /// Returns `false` for anything that might produce a heap-allocated
    /// value: variables (might hold heap pointers), non-intrinsic calls,
    /// lambdas, strings, quotes, etc.
    pub(super) fn result_is_safe(&self, hir: &Hir) -> bool {
        match &hir.kind {
            // Literals: all NaN-boxed immediates
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList => true,

            // Control flow: recurse into all result positions
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => self.result_is_safe(then_branch) && self.result_is_safe(else_branch),

            HirKind::Begin(exprs) => {
                // Empty begin produces nil (an immediate)
                match exprs.last() {
                    Some(last) => self.result_is_safe(last),
                    None => true,
                }
            }

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                // All clause bodies must be safe
                let clauses_safe = clauses.iter().all(|(_, body)| self.result_is_safe(body));
                // Missing else produces nil (safe); present else must be safe
                let else_safe = match else_branch {
                    Some(branch) => self.result_is_safe(branch),
                    None => true,
                };
                clauses_safe && else_safe
            }

            HirKind::And(exprs) | HirKind::Or(exprs) => {
                // Short-circuit: any sub-expression could be the result
                exprs.iter().all(|e| self.result_is_safe(e))
            }

            // Intrinsic calls that return immediates
            HirKind::Call { func, args, .. } => self.call_result_is_safe(func, args),

            // Everything else: conservatively unsafe
            // String, Var, Lambda, Let, Letrec, Block, While, Match,
            // Yield, Quote, Eval, Set, Define, Destructure, Break
            _ => false,
        }
    }

    /// Check if a function call is to a known intrinsic or immediate-returning
    /// primitive, meaning its result is guaranteed to be an immediate.
    fn call_result_is_safe(&self, func: &Hir, args: &[CallArg]) -> bool {
        // Must be a variable reference to a global
        let HirKind::Var(binding) = &func.kind else {
            return false;
        };

        // Must be a non-mutated global (same check as try_lower_intrinsic)
        if !binding.is_global() || binding.is_mutated() {
            return false;
        }

        // Any spliced argument means generic CallArray, not intrinsic
        if args.iter().any(|a| a.spliced) {
            return false;
        }

        let sym = binding.name();

        // Check intrinsics map (BinOp, CmpOp, UnaryOp with correct arity)
        if let Some(op) = self.intrinsics.get(&sym) {
            return match op {
                IntrinsicOp::Binary(_) | IntrinsicOp::Compare(_) => args.len() == 2,
                IntrinsicOp::Unary(_) => args.len() == 1,
            };
        }

        // Check immediate-returning primitives whitelist.
        // No arity check needed — wrong arity produces SIG_ERROR which
        // propagates via the signal mechanism, never as a heap return value.
        self.immediate_primitives.contains(&sym)
    }

    /// Check if a HIR body contains any `set!` to a binding that is NOT
    /// in the given set of scope-local bindings.
    ///
    /// An outward `set!` stores a value (possibly heap-allocated inside
    /// the scope) into a binding that outlives the scope. After
    /// `RegionExit`, that binding holds a dangling pointer.
    ///
    /// Recursion rules:
    /// - Recurses into all sub-expressions.
    /// - Does NOT recurse into `Lambda` bodies (separate scope; captures
    ///   caught by condition 1).
    /// - DOES recurse into nested `Let`/`Letrec`/`Block` bodies (part of
    ///   the current execution flow).
    pub(super) fn body_contains_outward_set(hir: &Hir, scope_bindings: &[Binding]) -> bool {
        Self::walk_for_outward_set(hir, scope_bindings)
    }

    fn walk_for_outward_set(hir: &Hir, scope_bindings: &[Binding]) -> bool {
        match &hir.kind {
            HirKind::Set { target, value } => {
                // Check if target is outside our scope
                if !scope_bindings.contains(target) {
                    return true;
                }
                Self::walk_for_outward_set(value, scope_bindings)
            }

            // Do NOT recurse into lambda bodies
            HirKind::Lambda { .. } => false,

            // Recurse into all sub-expressions for other node types
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList
            | HirKind::String(_) => false,

            HirKind::Var(_) => false,

            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                Self::walk_for_outward_set(cond, scope_bindings)
                    || Self::walk_for_outward_set(then_branch, scope_bindings)
                    || Self::walk_for_outward_set(else_branch, scope_bindings)
            }

            HirKind::Begin(exprs) => exprs
                .iter()
                .any(|e| Self::walk_for_outward_set(e, scope_bindings)),

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses.iter().any(|(cond, body)| {
                    Self::walk_for_outward_set(cond, scope_bindings)
                        || Self::walk_for_outward_set(body, scope_bindings)
                }) || else_branch
                    .as_ref()
                    .is_some_and(|b| Self::walk_for_outward_set(b, scope_bindings))
            }

            HirKind::And(exprs) | HirKind::Or(exprs) => exprs
                .iter()
                .any(|e| Self::walk_for_outward_set(e, scope_bindings)),

            HirKind::Call { func, args, .. } => {
                Self::walk_for_outward_set(func, scope_bindings)
                    || args
                        .iter()
                        .any(|a| Self::walk_for_outward_set(&a.expr, scope_bindings))
            }

            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                bindings
                    .iter()
                    .any(|(_, init)| Self::walk_for_outward_set(init, scope_bindings))
                    || Self::walk_for_outward_set(body, scope_bindings)
            }

            HirKind::Define { value, .. } => Self::walk_for_outward_set(value, scope_bindings),

            HirKind::While { cond, body } => {
                Self::walk_for_outward_set(cond, scope_bindings)
                    || Self::walk_for_outward_set(body, scope_bindings)
            }

            HirKind::Block { body, .. } => body
                .iter()
                .any(|e| Self::walk_for_outward_set(e, scope_bindings)),

            HirKind::Break { value, .. } => Self::walk_for_outward_set(value, scope_bindings),

            HirKind::Match { value, arms } => {
                Self::walk_for_outward_set(value, scope_bindings)
                    || arms.iter().any(|(_, guard, body)| {
                        guard
                            .as_ref()
                            .is_some_and(|g| Self::walk_for_outward_set(g, scope_bindings))
                            || Self::walk_for_outward_set(body, scope_bindings)
                    })
            }

            HirKind::Yield(expr) => Self::walk_for_outward_set(expr, scope_bindings),

            HirKind::Quote(_) => false,

            HirKind::Destructure { value, .. } => Self::walk_for_outward_set(value, scope_bindings),

            HirKind::Eval { expr, env } => {
                Self::walk_for_outward_set(expr, scope_bindings)
                    || Self::walk_for_outward_set(env, scope_bindings)
            }
        }
    }

    /// Check if a HIR body (slice) contains any `Break` node.
    ///
    /// Used by block scope allocation to conservatively reject blocks
    /// with breaks (break values might be heap-allocated).
    pub(super) fn body_contains_break(body: &[Hir]) -> bool {
        body.iter().any(Self::walk_for_break)
    }

    /// Check if a single HIR expression contains any `Break` node.
    ///
    /// Used by let/letrec scope allocation: a break inside the let body
    /// carries a value past the compensating `RegionExit`, which would
    /// free scope-allocated objects the break value might reference.
    pub(super) fn hir_contains_break(hir: &Hir) -> bool {
        Self::walk_for_break(hir)
    }

    /// Recursion rules:
    /// - Does NOT recurse into `Lambda` bodies (break can't cross fn boundaries).
    /// - DOES recurse into nested `Block` bodies (a break inside a nested
    ///   block might target an outer block, escaping our scope).
    fn walk_for_break(hir: &Hir) -> bool {
        match &hir.kind {
            HirKind::Break { .. } => true,

            // Do NOT recurse into lambda bodies
            HirKind::Lambda { .. } => false,

            // DO recurse into nested block bodies: a break inside a nested
            // block can target an outer block, carrying a value past our
            // scope's RegionExit.
            HirKind::Block { body, .. } => body.iter().any(Self::walk_for_break),

            // Terminals
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList
            | HirKind::String(_)
            | HirKind::Var(_)
            | HirKind::Quote(_) => false,

            // Recurse into sub-expressions
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                Self::walk_for_break(cond)
                    || Self::walk_for_break(then_branch)
                    || Self::walk_for_break(else_branch)
            }

            HirKind::Begin(exprs) | HirKind::And(exprs) | HirKind::Or(exprs) => {
                exprs.iter().any(Self::walk_for_break)
            }

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses
                    .iter()
                    .any(|(c, b)| Self::walk_for_break(c) || Self::walk_for_break(b))
                    || else_branch.as_deref().is_some_and(Self::walk_for_break)
            }

            HirKind::Call { func, args, .. } => {
                Self::walk_for_break(func) || args.iter().any(|a| Self::walk_for_break(&a.expr))
            }

            HirKind::Set { value, .. } | HirKind::Define { value, .. } => {
                Self::walk_for_break(value)
            }

            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                bindings.iter().any(|(_, init)| Self::walk_for_break(init))
                    || Self::walk_for_break(body)
            }

            HirKind::While { cond, body } => {
                Self::walk_for_break(cond) || Self::walk_for_break(body)
            }

            HirKind::Match { value, arms } => {
                Self::walk_for_break(value)
                    || arms.iter().any(|(_, guard, body)| {
                        guard.as_ref().is_some_and(Self::walk_for_break)
                            || Self::walk_for_break(body)
                    })
            }

            HirKind::Yield(expr) => Self::walk_for_break(expr),

            HirKind::Destructure { value, .. } => Self::walk_for_break(value),

            HirKind::Eval { expr, env } => Self::walk_for_break(expr) || Self::walk_for_break(env),
        }
    }
}
