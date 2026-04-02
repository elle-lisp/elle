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
//! 3. Body result is provably an immediate (`result_is_safe`)
//! 4. Body contains no dangerous `set` to bindings outside this scope
//!    (`body_contains_dangerous_outward_set`)
//! 5. All breaks in body carry safe immediate values
//!    (`all_breaks_have_safe_values` / `all_break_values_safe`) — Tier 6
//! 6. Body contains no escaping `break` (`hir_contains_escaping_break`) —
//!    a break targeting an outer block carries a value past `RegionExit`.
//!    Breaks targeting blocks inside the scope are safe (Tier 7).
//!
//! All six conditions must be satisfied for scope allocation to be safe.
//!
//! ## What `RegionExit` frees
//!
//! `RegionExit` runs destructors for ALL heap objects allocated between
//! `RegionEnter` and `RegionExit` — including objects the body allocated
//! (not just binding values). This is why condition 3 is required: the
//! body's result, if heap-allocated inside the scope, gets freed before
//! the caller uses it.

use std::collections::HashSet;

use super::Lowerer;
use crate::hir::{Binding, BlockId, CallArg, Hir, HirKind, HirPattern};
use crate::lir::intrinsics::IntrinsicOp;
use crate::lir::types::BinOp;

impl<'a> Lowerer<'a> {
    /// Check if the result of a HIR expression is provably an immediate
    /// (not a heap pointer to something allocated inside the scope).
    ///
    /// `scope_bindings` contains the bindings introduced by the let/letrec
    /// being analyzed. A `Var` referencing a binding NOT in this set is
    /// safe to return (the value was allocated before the scope's
    /// `RegionEnter`). A `Var` referencing a scope binding is safe only
    /// if its init expression is itself provably immediate.
    ///
    /// Returns `true` for: literals, intrinsic/whitelisted calls, Var
    /// references to outer bindings (or scope bindings with immediate inits),
    /// and control flow where all result positions are safe.
    ///
    /// Returns `false` for anything that might produce a heap-allocated
    /// value: non-intrinsic calls, lambdas, strings, quotes, etc.
    pub(super) fn result_is_safe(&self, hir: &Hir, scope_bindings: &[(Binding, &Hir)]) -> bool {
        match &hir.kind {
            // Literals: all immediates
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList => true,

            // Var: safe if binding is from outside the scope (value was
            // allocated before RegionEnter) or if the binding is in-scope
            // but its init expression is provably immediate.
            HirKind::Var(binding) => {
                match scope_bindings.iter().find(|(b, _)| b == binding) {
                    None => true, // outer binding — safe
                    Some((_, init)) => self.result_is_safe(init, scope_bindings),
                }
            }

            // Control flow: recurse into all result positions
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.result_is_safe(then_branch, scope_bindings)
                    && self.result_is_safe(else_branch, scope_bindings)
            }

            HirKind::Begin(exprs) => {
                // Empty begin produces nil (an immediate)
                let Some(last) = exprs.last() else {
                    return true;
                };
                // Destructure nodes that precede the last expression bind
                // variables to values produced by StructRest and similar
                // operations — all of which are heap-allocated inside the
                // current scope. Track these bindings so that a Var
                // referencing one is not mistaken for an outer (pre-scope)
                // binding.
                //
                // We use a sentinel Hir whose result_is_safe is false,
                // representing "heap-allocated inside the scope".
                // Yield is always unsafe in result_is_safe.
                let sentinel = Hir::silent(
                    HirKind::Yield(Box::new(Hir::silent(
                        HirKind::Nil,
                        crate::syntax::Span::synthetic(),
                    ))),
                    crate::syntax::Span::synthetic(),
                );
                let mut extended: Vec<(Binding, &Hir)> = scope_bindings.to_vec();
                for expr in &exprs[..exprs.len() - 1] {
                    match &expr.kind {
                        HirKind::Destructure { pattern, .. } => {
                            collect_destructure_bindings(pattern, &sentinel, &mut extended);
                        }
                        HirKind::Define { binding, value } => {
                            extended.push((*binding, value));
                        }
                        _ => {}
                    }
                }
                self.result_is_safe(last, &extended)
            }

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                // All clause bodies must be safe
                let clauses_safe = clauses
                    .iter()
                    .all(|(_, body)| self.result_is_safe(body, scope_bindings));
                // Missing else produces nil (safe); present else must be safe
                let else_safe = match else_branch {
                    Some(branch) => self.result_is_safe(branch, scope_bindings),
                    None => true,
                };
                clauses_safe && else_safe
            }

            HirKind::And(exprs) | HirKind::Or(exprs) => {
                // Short-circuit: any sub-expression could be the result
                exprs.iter().all(|e| self.result_is_safe(e, scope_bindings))
            }

            // Tail call: replaces the frame, so the scope's allocations are
            // dead. Safe if neither the callee nor any argument references a
            // scope binding (which would mean a scope-allocated value is used
            // after RegionExit frees it — the callee is invoked and the args
            // are passed AFTER the scope exits).
            HirKind::Call {
                is_tail: true,
                func,
                args,
            } => {
                self.result_is_safe(func, scope_bindings)
                    && args
                        .iter()
                        .all(|a| self.result_is_safe(&a.expr, scope_bindings))
            }

            // Non-tail calls that return immediates
            HirKind::Call { func, args, .. } => self.call_result_is_safe(func, args),

            // Nested let/letrec: the result is the body's result.
            // Extend scope_bindings with the inner let's bindings so that
            // Var references to inner bindings are correctly checked against
            // their init expressions (they're allocated inside the outer
            // scope's region and would be freed by RegionExit).
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                let mut extended: Vec<(Binding, &Hir)> = scope_bindings.to_vec();
                extended.extend(bindings.iter().map(|(b, init)| (*b, init)));
                self.result_is_safe(body, &extended)
            }

            // Nested block: the result is either the last expression or a
            // break value targeting this block. Both must be safe.
            // Blocks introduce no bindings, so scope_bindings is unchanged.
            HirKind::Block { block_id, body, .. } => {
                let last_safe = match body.last() {
                    Some(last) => self.result_is_safe(last, scope_bindings),
                    None => true, // empty block → nil → safe
                };
                last_safe && self.all_break_values_safe(body, *block_id, scope_bindings)
            }

            // Match: all arm bodies must produce safe results.
            // Exactly one arm executes, analogous to If/Cond.
            HirKind::Match { arms, .. } => arms
                .iter()
                .all(|(_, _, body)| self.result_is_safe(body, scope_bindings)),

            // While always returns nil (an immediate).
            HirKind::While { .. } => true,

            // Destructure always returns nil (an immediate).
            HirKind::Destructure { .. } => true,

            // Break transfers control to the target block — the expression
            // itself never produces a value in the normal flow. The break
            // value's safety is checked separately by `all_break_values_safe`
            // or `all_breaks_have_safe_values`. Returning `true` here means
            // "this expression won't produce a heap value that escapes via
            // the normal return path."
            HirKind::Break { .. } => true,

            // Parameterize: result is the body's result
            HirKind::Parameterize { body, .. } => self.result_is_safe(body, scope_bindings),

            // String constants live in the constant pool (LoadConst),
            // not on the fiber heap. Safe to return from a scope.
            HirKind::String(_) => true,

            // Everything else: conservatively unsafe
            // Lambda, Yield, Quote, Eval, Set, Define
            _ => false,
        }
    }

    /// Check if a function call is to a known intrinsic or immediate-returning
    /// primitive/user function, meaning its result is guaranteed to be an immediate.
    pub(super) fn call_result_is_safe(&self, func: &Hir, args: &[CallArg]) -> bool {
        // Must be a variable reference
        let HirKind::Var(binding) = &func.kind else {
            return false;
        };

        // Check precomputed map first — works for letrec bindings which
        // are technically mutable (two-phase init) but whose result type
        // is determined by fixpoint analysis.
        // Note: callee_result_immediate is NOT checked here.
        // call_result_is_safe is used by result_is_safe which gates
        // let-scope allocation (conditions 3 and 4). Including user
        // functions here would change let-scope decisions globally
        // (e.g. stdlib functions like fold). call_result_is_safe
        // remains conservative: only intrinsics and whitelisted
        // primitives. The callee_result_immediate map is used only
        // by can_scope_allocate_call for targeted call-scoped regions.

        // Must be an immutable, non-mutated binding for intrinsic/primitive checks
        let bi = self.arena.get(*binding);
        if !bi.is_immutable || bi.is_mutated {
            return false;
        }

        // Any spliced argument means generic CallArrayMut, not intrinsic
        if args.iter().any(|a| a.spliced) {
            return false;
        }

        let sym = bi.name;

        // Check intrinsics map (BinOp, CmpOp, UnaryOp with correct arity).
        // Special case: `-` with 1 arg is negation (UnaryOp::Neg), which
        // returns an immediate, mirroring try_lower_intrinsic's special case.
        if let Some(op) = self.intrinsics.get(&sym) {
            return match op {
                IntrinsicOp::Binary(BinOp::Sub) => args.len() == 2 || args.len() == 1,
                IntrinsicOp::Binary(_) | IntrinsicOp::Compare(_) => args.len() == 2,
                IntrinsicOp::Unary(_) => args.len() == 1,
            };
        }

        // Check immediate-returning primitives whitelist.
        // No arity check needed — wrong arity produces SIG_ERROR which
        // propagates via the signal mechanism, never as a heap return value.
        self.immediate_primitives.contains(&sym)
    }

    /// Check if a function call is to a known built-in primitive.
    /// Built-in primitives do not internally create heap objects that
    /// escape to external mutable structures — they only produce return
    /// values and/or mutate their arguments (caught separately).
    fn callee_is_primitive(&self, func: &Hir) -> bool {
        let HirKind::Var(binding) = &func.kind else {
            return false;
        };
        if let Some(val) = self.immutable_values.get(binding) {
            return val.is_native_fn();
        }
        false
    }

    /// Check if a HIR body contains any dangerous `set!` to a binding
    /// outside the scope.
    ///
    /// A `set!` to an outer binding is dangerous only if the assigned
    /// value could be heap-allocated inside the scope. If the value is
    /// provably immediate (via `result_is_safe`), the outer binding
    /// receives an immediate that won't dangle after `RegionExit`.
    ///
    /// `scope_bindings` contains both the binding identity AND init
    /// expressions, used by `result_is_safe` when the assigned value
    /// references a scope binding.
    ///
    /// Recursion rules:
    /// - Recurses into all sub-expressions.
    /// - Does NOT recurse into `Lambda` bodies (separate scope; captures
    ///   caught by condition 1).
    /// - DOES recurse into nested `Let`/`Letrec`/`Block` bodies (part of
    ///   the current execution flow).
    /// - When entering nested `Let`/`Letrec`, extends `scope_bindings`
    ///   with the inner let's bindings so inner mutations are not
    ///   treated as outward.
    pub(super) fn body_contains_dangerous_outward_set(
        &self,
        hir: &Hir,
        scope_bindings: &[(Binding, &Hir)],
    ) -> bool {
        self.walk_for_outward_set(hir, scope_bindings)
    }

    fn walk_for_outward_set(&self, hir: &Hir, scope_bindings: &[(Binding, &Hir)]) -> bool {
        match &hir.kind {
            HirKind::Assign { target, value } => {
                // Check if target is outside our scope
                let in_scope = scope_bindings.iter().any(|(b, _)| b == target);
                if !in_scope {
                    // Outward assign — only dangerous if value could be heap-allocated
                    if !self.result_is_safe(value, scope_bindings) {
                        return true;
                    }
                }
                // Recurse into value expression (it may contain further assigns)
                self.walk_for_outward_set(value, scope_bindings)
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
                self.walk_for_outward_set(cond, scope_bindings)
                    || self.walk_for_outward_set(then_branch, scope_bindings)
                    || self.walk_for_outward_set(else_branch, scope_bindings)
            }

            HirKind::Begin(exprs) => exprs
                .iter()
                .any(|e| self.walk_for_outward_set(e, scope_bindings)),

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses.iter().any(|(cond, body)| {
                    self.walk_for_outward_set(cond, scope_bindings)
                        || self.walk_for_outward_set(body, scope_bindings)
                }) || else_branch
                    .as_ref()
                    .is_some_and(|b| self.walk_for_outward_set(b, scope_bindings))
            }

            HirKind::And(exprs) | HirKind::Or(exprs) => exprs
                .iter()
                .any(|e| self.walk_for_outward_set(e, scope_bindings)),

            HirKind::Call {
                func,
                args,
                is_tail,
            } => {
                // Tail calls replace the frame — the callee runs in a new
                // context and cannot store scope-allocated values externally
                // (the scope is gone by the time the callee executes). The
                // only danger is scope-allocated values flowing into args,
                // which is caught by condition 3 (result_is_safe).
                if !*is_tail {
                    let callee_is_safe = self.call_result_is_safe(func, args);
                    if !callee_is_safe {
                        // Check 1: any non-safe callee receiving a heap-allocated
                        // scope-local argument may store it externally (e.g. push
                        // into an outer @array).
                        if args
                            .iter()
                            .any(|a| !self.result_is_safe(&a.expr, scope_bindings))
                        {
                            return true;
                        }
                        // Check 2: user-defined functions (non-primitives) may
                        // internally allocate heap objects and store them in
                        // external mutable structures (e.g. via put to an outer
                        // @struct). Built-in primitives are safe — they only
                        // produce return values and/or mutate their arguments
                        // (caught by check 1).
                        //
                        // Safe if the callee is a built-in primitive (only
                        // produces return values / mutates args, caught by
                        // check 1) OR rotation-safe (proven not to escape
                        // heap values to external structures). Rotation-safety
                        // transitively checks for internal allocations stored
                        // externally via mutating primitives.
                        if !self.callee_is_primitive(func) && !self.callee_is_rotation_safe(func) {
                            return true;
                        }
                    }
                }
                self.walk_for_outward_set(func, scope_bindings)
                    || args
                        .iter()
                        .any(|a| self.walk_for_outward_set(&a.expr, scope_bindings))
            }

            // Nested let/letrec: extend scope_bindings with inner bindings
            // so mutations to inner bindings are not treated as outward.
            // Init expressions are walked with the OUTER scope (inner bindings
            // don't exist yet during init evaluation).
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                // Walk inits with current scope
                if bindings
                    .iter()
                    .any(|(_, init)| self.walk_for_outward_set(init, scope_bindings))
                {
                    return true;
                }
                // Walk body with extended scope
                let mut extended: Vec<(Binding, &Hir)> = scope_bindings.to_vec();
                extended.extend(bindings.iter().map(|(b, init)| (*b, init)));
                self.walk_for_outward_set(body, &extended)
            }

            HirKind::Define { value, .. } => self.walk_for_outward_set(value, scope_bindings),

            HirKind::While { cond, body } => {
                self.walk_for_outward_set(cond, scope_bindings)
                    || self.walk_for_outward_set(body, scope_bindings)
            }

            HirKind::Block { body, .. } => body
                .iter()
                .any(|e| self.walk_for_outward_set(e, scope_bindings)),

            HirKind::Break { value, .. } => self.walk_for_outward_set(value, scope_bindings),

            HirKind::Match { value, arms } => {
                self.walk_for_outward_set(value, scope_bindings)
                    || arms.iter().any(|(_, guard, body)| {
                        guard
                            .as_ref()
                            .is_some_and(|g| self.walk_for_outward_set(g, scope_bindings))
                            || self.walk_for_outward_set(body, scope_bindings)
                    })
            }

            HirKind::Yield(expr) => self.walk_for_outward_set(expr, scope_bindings),

            HirKind::Quote(_) => false,

            HirKind::Destructure { value, .. } => self.walk_for_outward_set(value, scope_bindings),

            HirKind::Eval { expr, env } => {
                self.walk_for_outward_set(expr, scope_bindings)
                    || self.walk_for_outward_set(env, scope_bindings)
            }

            HirKind::Parameterize { bindings, body } => {
                bindings.iter().any(|(param, value)| {
                    self.walk_for_outward_set(param, scope_bindings)
                        || self.walk_for_outward_set(value, scope_bindings)
                }) || self.walk_for_outward_set(body, scope_bindings)
            }
        }
    }

    /// Check that every `Break` targeting `target_id` within the given body
    /// has a value that is provably an immediate.
    ///
    /// `scope_bindings` tracks bindings introduced by let/letrec nodes
    /// between the block and the break site. This is critical for blocks:
    /// a break value that references a let binding with a heap init is
    /// unsafe even though the Var looks "outer" relative to the block
    /// (which has no bindings of its own).
    ///
    /// Recursion rules:
    /// - Does NOT recurse into `Lambda` bodies (breaks can't cross fn boundaries).
    /// - DOES recurse into nested `Block` bodies (a break inside a nested
    ///   block may target the outer block). `BlockId`s are unique, so a
    ///   nested block never shadows the target.
    /// - When entering `Let`/`Letrec`, extends `scope_bindings` with the
    ///   inner bindings so break values referencing them are checked against
    ///   their init expressions.
    pub(super) fn all_break_values_safe(
        &self,
        body: &[Hir],
        target_id: BlockId,
        scope_bindings: &[(Binding, &Hir)],
    ) -> bool {
        body.iter()
            .all(|hir| self.hir_break_values_safe(hir, target_id, scope_bindings))
    }

    fn hir_break_values_safe(
        &self,
        hir: &Hir,
        target_id: BlockId,
        scope_bindings: &[(Binding, &Hir)],
    ) -> bool {
        match &hir.kind {
            HirKind::Break { block_id, value } => {
                if *block_id == target_id {
                    self.result_is_safe(value, scope_bindings)
                } else {
                    // Targets a different block — still recurse into value
                    // (value expr might contain a break targeting us)
                    self.hir_break_values_safe(value, target_id, scope_bindings)
                }
            }

            // Do NOT recurse into lambdas
            HirKind::Lambda { .. } => true,

            // Terminals — no breaks possible
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList
            | HirKind::String(_)
            | HirKind::Var(_)
            | HirKind::Quote(_) => true,

            // Recurse into sub-expressions
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.hir_break_values_safe(cond, target_id, scope_bindings)
                    && self.hir_break_values_safe(then_branch, target_id, scope_bindings)
                    && self.hir_break_values_safe(else_branch, target_id, scope_bindings)
            }

            HirKind::Begin(exprs) | HirKind::And(exprs) | HirKind::Or(exprs) => exprs
                .iter()
                .all(|e| self.hir_break_values_safe(e, target_id, scope_bindings)),

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses.iter().all(|(c, b)| {
                    self.hir_break_values_safe(c, target_id, scope_bindings)
                        && self.hir_break_values_safe(b, target_id, scope_bindings)
                }) && else_branch
                    .as_deref()
                    .is_none_or(|b| self.hir_break_values_safe(b, target_id, scope_bindings))
            }

            HirKind::Call { func, args, .. } => {
                self.hir_break_values_safe(func, target_id, scope_bindings)
                    && args
                        .iter()
                        .all(|a| self.hir_break_values_safe(&a.expr, target_id, scope_bindings))
            }

            HirKind::Assign { value, .. } | HirKind::Define { value, .. } => {
                self.hir_break_values_safe(value, target_id, scope_bindings)
            }

            // Nested let/letrec: extend scope_bindings with inner bindings.
            // Init expressions are walked with the OUTER scope (inner bindings
            // don't exist yet during init evaluation).
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                if !bindings
                    .iter()
                    .all(|(_, init)| self.hir_break_values_safe(init, target_id, scope_bindings))
                {
                    return false;
                }
                let mut extended: Vec<(Binding, &Hir)> = scope_bindings.to_vec();
                extended.extend(bindings.iter().map(|(b, init)| (*b, init)));
                self.hir_break_values_safe(body, target_id, &extended)
            }

            HirKind::While { cond, body } => {
                self.hir_break_values_safe(cond, target_id, scope_bindings)
                    && self.hir_break_values_safe(body, target_id, scope_bindings)
            }

            HirKind::Block { body, .. } => body
                .iter()
                .all(|e| self.hir_break_values_safe(e, target_id, scope_bindings)),

            HirKind::Match { value, arms } => {
                self.hir_break_values_safe(value, target_id, scope_bindings)
                    && arms.iter().all(|(_, guard, body)| {
                        guard.as_ref().is_none_or(|g| {
                            self.hir_break_values_safe(g, target_id, scope_bindings)
                        }) && self.hir_break_values_safe(body, target_id, scope_bindings)
                    })
            }

            HirKind::Yield(expr) => self.hir_break_values_safe(expr, target_id, scope_bindings),

            HirKind::Destructure { value, .. } => {
                self.hir_break_values_safe(value, target_id, scope_bindings)
            }

            HirKind::Eval { expr, env } => {
                self.hir_break_values_safe(expr, target_id, scope_bindings)
                    && self.hir_break_values_safe(env, target_id, scope_bindings)
            }

            HirKind::Parameterize { bindings, body } => {
                bindings.iter().all(|(param, value)| {
                    self.hir_break_values_safe(param, target_id, scope_bindings)
                        && self.hir_break_values_safe(value, target_id, scope_bindings)
                }) && self.hir_break_values_safe(body, target_id, scope_bindings)
            }
        }
    }

    /// Check if a single HIR expression contains a `Break` that escapes.
    ///
    /// A break "escapes" if it targets a block NOT defined within the
    /// expression. Used by `can_scope_allocate_let`: if the let body
    /// contains an escaping break, the break jumps past the let's
    /// `RegionExit`, so scope allocation is unsafe.
    ///
    /// Breaks targeting blocks defined INSIDE the expression are safe —
    /// they stay within the scope's region.
    ///
    /// Recursion rules:
    /// - Does NOT recurse into `Lambda` bodies (break can't cross fn boundaries).
    /// - DOES recurse into nested `Block` bodies, registering their `BlockId`
    ///   so that inner breaks targeting them are recognized as safe.
    pub(super) fn hir_contains_escaping_break(hir: &Hir) -> bool {
        let mut inner_blocks = HashSet::new();
        Self::walk_for_escaping_break(hir, &mut inner_blocks)
    }

    fn walk_for_escaping_break(hir: &Hir, inner_blocks: &mut HashSet<BlockId>) -> bool {
        match &hir.kind {
            HirKind::Break { block_id, .. } => {
                // Safe if targeting a block inside the scope
                !inner_blocks.contains(block_id)
            }

            // Do NOT recurse into lambda bodies
            HirKind::Lambda { .. } => false,

            // Register inner block's ID before recursing into its body.
            HirKind::Block { block_id, body, .. } => {
                inner_blocks.insert(*block_id);
                body.iter()
                    .any(|e| Self::walk_for_escaping_break(e, inner_blocks))
            }

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
                Self::walk_for_escaping_break(cond, inner_blocks)
                    || Self::walk_for_escaping_break(then_branch, inner_blocks)
                    || Self::walk_for_escaping_break(else_branch, inner_blocks)
            }

            HirKind::Begin(exprs) | HirKind::And(exprs) | HirKind::Or(exprs) => exprs
                .iter()
                .any(|e| Self::walk_for_escaping_break(e, inner_blocks)),

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses.iter().any(|(c, b)| {
                    Self::walk_for_escaping_break(c, inner_blocks)
                        || Self::walk_for_escaping_break(b, inner_blocks)
                }) || else_branch
                    .as_deref()
                    .is_some_and(|b| Self::walk_for_escaping_break(b, inner_blocks))
            }

            HirKind::Call { func, args, .. } => {
                Self::walk_for_escaping_break(func, inner_blocks)
                    || args
                        .iter()
                        .any(|a| Self::walk_for_escaping_break(&a.expr, inner_blocks))
            }

            HirKind::Assign { value, .. } | HirKind::Define { value, .. } => {
                Self::walk_for_escaping_break(value, inner_blocks)
            }

            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                bindings
                    .iter()
                    .any(|(_, init)| Self::walk_for_escaping_break(init, inner_blocks))
                    || Self::walk_for_escaping_break(body, inner_blocks)
            }

            HirKind::While { cond, body } => {
                Self::walk_for_escaping_break(cond, inner_blocks)
                    || Self::walk_for_escaping_break(body, inner_blocks)
            }

            HirKind::Match { value, arms } => {
                Self::walk_for_escaping_break(value, inner_blocks)
                    || arms.iter().any(|(_, guard, body)| {
                        guard
                            .as_ref()
                            .is_some_and(|g| Self::walk_for_escaping_break(g, inner_blocks))
                            || Self::walk_for_escaping_break(body, inner_blocks)
                    })
            }

            HirKind::Yield(expr) => Self::walk_for_escaping_break(expr, inner_blocks),

            HirKind::Destructure { value, .. } => {
                Self::walk_for_escaping_break(value, inner_blocks)
            }

            HirKind::Eval { expr, env } => {
                Self::walk_for_escaping_break(expr, inner_blocks)
                    || Self::walk_for_escaping_break(env, inner_blocks)
            }

            HirKind::Parameterize { bindings, body } => {
                bindings.iter().any(|(param, value)| {
                    Self::walk_for_escaping_break(param, inner_blocks)
                        || Self::walk_for_escaping_break(value, inner_blocks)
                }) || Self::walk_for_escaping_break(body, inner_blocks)
            }
        }
    }

    /// Check that every `Break` node reachable from this HIR expression
    /// has a value that is provably an immediate.
    ///
    /// Used by `can_scope_allocate_let`: if a break carries a heap value
    /// past the compensating RegionExit, the value dangles. If all break
    /// values are immediates, breaks are harmless regardless of target.
    ///
    /// Recursion: same as hir_break_values_safe (skip lambdas, enter nested blocks).
    pub(super) fn all_breaks_have_safe_values(&self, hir: &Hir) -> bool {
        match &hir.kind {
            HirKind::Break { value, .. } => self.result_is_safe(value, &[]),

            // Do NOT recurse into lambda bodies
            HirKind::Lambda { .. } => true,

            // Terminals
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList
            | HirKind::String(_)
            | HirKind::Var(_)
            | HirKind::Quote(_) => true,

            HirKind::Block { body, .. } => body.iter().all(|e| self.all_breaks_have_safe_values(e)),

            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.all_breaks_have_safe_values(cond)
                    && self.all_breaks_have_safe_values(then_branch)
                    && self.all_breaks_have_safe_values(else_branch)
            }

            HirKind::Begin(exprs) | HirKind::And(exprs) | HirKind::Or(exprs) => {
                exprs.iter().all(|e| self.all_breaks_have_safe_values(e))
            }

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses.iter().all(|(c, b)| {
                    self.all_breaks_have_safe_values(c) && self.all_breaks_have_safe_values(b)
                }) && else_branch
                    .as_deref()
                    .is_none_or(|b| self.all_breaks_have_safe_values(b))
            }

            HirKind::Call { func, args, .. } => {
                self.all_breaks_have_safe_values(func)
                    && args
                        .iter()
                        .all(|a| self.all_breaks_have_safe_values(&a.expr))
            }

            HirKind::Assign { value, .. } | HirKind::Define { value, .. } => {
                self.all_breaks_have_safe_values(value)
            }

            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                bindings
                    .iter()
                    .all(|(_, init)| self.all_breaks_have_safe_values(init))
                    && self.all_breaks_have_safe_values(body)
            }

            HirKind::While { cond, body } => {
                self.all_breaks_have_safe_values(cond) && self.all_breaks_have_safe_values(body)
            }

            HirKind::Match { value, arms } => {
                self.all_breaks_have_safe_values(value)
                    && arms.iter().all(|(_, guard, body)| {
                        guard
                            .as_ref()
                            .is_none_or(|g| self.all_breaks_have_safe_values(g))
                            && self.all_breaks_have_safe_values(body)
                    })
            }

            HirKind::Yield(expr) => self.all_breaks_have_safe_values(expr),

            HirKind::Destructure { value, .. } => self.all_breaks_have_safe_values(value),

            HirKind::Eval { expr, env } => {
                self.all_breaks_have_safe_values(expr) && self.all_breaks_have_safe_values(env)
            }

            HirKind::Parameterize { bindings, body } => {
                bindings.iter().all(|(param, value)| {
                    self.all_breaks_have_safe_values(param)
                        && self.all_breaks_have_safe_values(value)
                }) && self.all_breaks_have_safe_values(body)
            }
        }
    }

    /// Check if a HIR body (slice) contains a `Break` that escapes the scope.
    ///
    /// A break targeting a block INSIDE the body is safe — it stays within the
    /// scope's region and RegionExit still fires on the normal exit path.
    /// Only breaks targeting blocks OUTSIDE the body are dangerous (they jump
    /// past the scope's RegionExit).
    #[allow(dead_code)]
    pub(super) fn body_contains_escaping_break(body: &[Hir]) -> bool {
        let mut inner_blocks = HashSet::new();
        body.iter()
            .any(|e| Self::walk_for_escaping_break(e, &mut inner_blocks))
    }
}

/// Collect all `Var` bindings introduced by a `Destructure` pattern into
/// `out`, each paired with `sentinel` as its "init" expression.
///
/// Used by `result_is_safe` to track pattern-bound variables produced by
/// `Destructure` nodes within a `Begin` body. These bindings receive heap-
/// allocated values (e.g. from `StructRest`) inside the current scope, so
/// they are not safe to return from a scope-allocated `let`.
fn collect_destructure_bindings<'a>(
    pattern: &HirPattern,
    sentinel: &'a Hir,
    out: &mut Vec<(Binding, &'a Hir)>,
) {
    match pattern {
        HirPattern::Var(b) => out.push((*b, sentinel)),
        HirPattern::Wildcard | HirPattern::Nil | HirPattern::Literal(_) => {}
        HirPattern::Cons { head, tail } => {
            collect_destructure_bindings(head, sentinel, out);
            collect_destructure_bindings(tail, sentinel, out);
        }
        HirPattern::List { elements, rest }
        | HirPattern::Tuple { elements, rest }
        | HirPattern::Array { elements, rest } => {
            for p in elements {
                collect_destructure_bindings(p, sentinel, out);
            }
            if let Some(r) = rest {
                collect_destructure_bindings(r, sentinel, out);
            }
        }
        HirPattern::Struct { entries, rest } | HirPattern::Table { entries, rest } => {
            for (_, p) in entries {
                collect_destructure_bindings(p, sentinel, out);
            }
            if let Some(r) = rest {
                collect_destructure_bindings(r, sentinel, out);
            }
        }
        HirPattern::NamedStruct { entries } => {
            for (_, p) in entries {
                collect_destructure_bindings(p, sentinel, out);
            }
        }
        HirPattern::Set { binding } | HirPattern::SetMut { binding } => {
            collect_destructure_bindings(binding, sentinel, out);
        }
        HirPattern::Or(alternatives) => {
            // All alternatives bind the same variables; collect from the first
            if let Some(first) = alternatives.first() {
                collect_destructure_bindings(first, sentinel, out);
            }
        }
    }
}

// ── Rotation-safety analysis ──────────────────────────────────────────

impl<'a> Lowerer<'a> {
    /// Walk the HIR looking for operations that escape heap values.
    /// Returns true if any escaping operation is found.
    ///
    /// An operation "escapes" when it stores a heap value into a data
    /// structure that outlives the current stack frame:
    /// - `assign` to a captured/global binding with a non-immediate value
    /// - Calls to mutating primitives (push, put, fiber/resume) with
    ///   non-immediate arguments
    /// - Calls to non-primitive functions (may internally do the above)
    /// - `yield` with a non-immediate value
    ///
    /// Tail-call sub-expressions are excluded: the tail call replaces
    /// the frame, so the callee runs in a new context.
    pub(super) fn body_escapes_heap_values(&self, hir: &Hir) -> bool {
        match &hir.kind {
            HirKind::Assign { value, .. } => {
                !self.result_is_safe(value, &[]) || self.body_escapes_heap_values(value)
            }
            HirKind::Call {
                func,
                args,
                is_tail,
            } => {
                // Mutating primitives escape even in tail position:
                // (put table key @{...}) stores a heap value externally
                // before returning, regardless of tail-call optimization.
                if self.callee_is_mutating_primitive(func)
                    && args.iter().any(|a| !self.result_is_safe(&a.expr, &[]))
                {
                    return true;
                }
                // Non-tail, non-mutating calls: check callee safety.
                // Tail calls to non-mutating callees are safe — the frame
                // is replaced and the callee runs in a new context.
                if *is_tail {
                    return false;
                }
                if !self.callee_is_primitive(func) && !self.callee_is_rotation_safe(func) {
                    return true;
                }
                self.body_escapes_heap_values(func)
                    || args.iter().any(|a| self.body_escapes_heap_values(&a.expr))
            }
            HirKind::Lambda { .. } => false,
            HirKind::Yield(value) => {
                !self.result_is_safe(value, &[]) || self.body_escapes_heap_values(value)
            }
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList
            | HirKind::String(_)
            | HirKind::Var(_) => false,
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.body_escapes_heap_values(cond)
                    || self.body_escapes_heap_values(then_branch)
                    || self.body_escapes_heap_values(else_branch)
            }
            HirKind::Begin(exprs) => exprs.iter().any(|e| self.body_escapes_heap_values(e)),
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses.iter().any(|(c, b)| {
                    self.body_escapes_heap_values(c) || self.body_escapes_heap_values(b)
                }) || else_branch
                    .as_ref()
                    .is_some_and(|b| self.body_escapes_heap_values(b))
            }
            HirKind::And(exprs) | HirKind::Or(exprs) => {
                exprs.iter().any(|e| self.body_escapes_heap_values(e))
            }
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                bindings
                    .iter()
                    .any(|(_, init)| self.body_escapes_heap_values(init))
                    || self.body_escapes_heap_values(body)
            }
            HirKind::Define { value, .. } => self.body_escapes_heap_values(value),
            HirKind::While { cond, body } => {
                self.body_escapes_heap_values(cond) || self.body_escapes_heap_values(body)
            }
            HirKind::Block { body, .. } => body.iter().any(|e| self.body_escapes_heap_values(e)),
            HirKind::Break { value, .. } => self.body_escapes_heap_values(value),
            HirKind::Match { value, arms } => {
                self.body_escapes_heap_values(value)
                    || arms
                        .iter()
                        .any(|(_, _, body)| self.body_escapes_heap_values(body))
            }
            HirKind::Parameterize { bindings, body } => {
                bindings
                    .iter()
                    .any(|(_, v)| self.body_escapes_heap_values(v))
                    || self.body_escapes_heap_values(body)
            }
            _ => true,
        }
    }

    /// Check if a callee is a known rotation-safe user function.
    /// Uses the `callee_rotation_safe` map populated during lowering.
    fn callee_is_rotation_safe(&self, func: &Hir) -> bool {
        let HirKind::Var(binding) = &func.kind else {
            return false;
        };
        self.callee_rotation_safe
            .get(binding)
            .copied()
            .unwrap_or(false)
    }

    fn callee_is_mutating_primitive(&self, func: &Hir) -> bool {
        let HirKind::Var(binding) = &func.kind else {
            return false;
        };
        let bi = self.arena.get(*binding);
        self.mutating_primitives.contains(&bi.name)
    }
}
