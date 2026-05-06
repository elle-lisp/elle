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
        self.result_is_safe_impl(hir, scope_bindings, false)
    }

    /// Extended version that also trusts calls to `callee_return_safe`
    /// functions. Used only by `precompute_return_safe` for fixpoint
    /// iteration — not for general scope allocation decisions.
    pub(super) fn result_is_safe_extended(
        &self,
        hir: &Hir,
        scope_bindings: &[(Binding, &Hir)],
    ) -> bool {
        self.result_is_safe_impl(hir, scope_bindings, true)
    }

    fn result_is_safe_impl(
        &self,
        hir: &Hir,
        scope_bindings: &[(Binding, &Hir)],
        trust_return_safe: bool,
    ) -> bool {
        match &hir.kind {
            // Literals and quotes: all immediates (constant pool)
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList
            | HirKind::Quote(_) => true,

            // Var: safe if binding is from outside the scope (value was
            // allocated before RegionEnter) or if the binding is in-scope
            // but its init expression is provably immediate.
            HirKind::Var(binding) => {
                match scope_bindings.iter().find(|(b, _)| b == binding) {
                    None => true, // outer binding — safe
                    Some((_, init)) => {
                        self.result_is_safe_impl(init, scope_bindings, trust_return_safe)
                    }
                }
            }
            // DerefCell: functionalize wraps letrec bindings in cells.
            // Treat like Var — the cell holds a pre-existing value.
            HirKind::DerefCell { cell } => {
                self.result_is_safe_impl(cell, scope_bindings, trust_return_safe)
            }

            // Control flow: recurse into all result positions
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.result_is_safe_impl(then_branch, scope_bindings, trust_return_safe)
                    && self.result_is_safe_impl(else_branch, scope_bindings, trust_return_safe)
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
                // Emit is always unsafe in result_is_safe.
                let sentinel = Hir::silent(
                    HirKind::Emit {
                        signal: crate::value::fiber::SIG_YIELD,
                        value: Box::new(Hir::silent(
                            HirKind::Nil,
                            crate::syntax::Span::synthetic(),
                        )),
                    },
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
                self.result_is_safe_impl(last, &extended, trust_return_safe)
            }

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                // All clause bodies must be safe
                let clauses_safe = clauses.iter().all(|(_, body)| {
                    self.result_is_safe_impl(body, scope_bindings, trust_return_safe)
                });
                // Missing else produces nil (safe); present else must be safe
                let else_safe = match else_branch {
                    Some(branch) => {
                        self.result_is_safe_impl(branch, scope_bindings, trust_return_safe)
                    }
                    None => true,
                };
                clauses_safe && else_safe
            }

            HirKind::And(exprs) | HirKind::Or(exprs) => {
                // Short-circuit: any sub-expression could be the result
                exprs
                    .iter()
                    .all(|e| self.result_is_safe_impl(e, scope_bindings, trust_return_safe))
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
                self.result_is_safe_impl(func, scope_bindings, trust_return_safe)
                    && args.iter().all(|a| {
                        self.result_is_safe_impl(&a.expr, scope_bindings, trust_return_safe)
                    })
            }

            // Non-tail calls that return immediates
            HirKind::Call { func, args, .. } => {
                if self.call_result_is_safe(func, args) {
                    return true;
                }
                // Extended mode: also trust calls to callee_return_safe functions.
                // These are user functions proven (by fixpoint) to never return
                // freshly heap-allocated values.
                // Look through DerefCell (functionalize wraps letrec bindings).
                if trust_return_safe {
                    let binding = match &func.kind {
                        HirKind::Var(b) => Some(b),
                        HirKind::DerefCell { cell } => match &cell.kind {
                            HirKind::Var(b) => Some(b),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(binding) = binding {
                        if self
                            .callee_return_safe
                            .get(binding)
                            .copied()
                            .unwrap_or(false)
                        {
                            return true;
                        }
                    }
                }
                false
            }

            // Nested let/letrec: the result is the body's result.
            // Extend scope_bindings with the inner let's bindings so that
            // Var references to inner bindings are correctly checked against
            // their init expressions (they're allocated inside the outer
            // scope's region and would be freed by RegionExit).
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                let mut extended: Vec<(Binding, &Hir)> = scope_bindings.to_vec();
                extended.extend(bindings.iter().map(|(b, init)| (*b, init)));
                self.result_is_safe_impl(body, &extended, trust_return_safe)
            }

            // Nested block: the result is either the last expression or a
            // break value targeting this block. Both must be safe.
            // Blocks introduce no bindings, so scope_bindings is unchanged.
            HirKind::Block { block_id, body, .. } => {
                let last_safe = match body.last() {
                    Some(last) => self.result_is_safe_impl(last, scope_bindings, trust_return_safe),
                    None => true, // empty block → nil → safe
                };
                last_safe && self.all_break_values_safe(body, *block_id, scope_bindings)
            }

            // Match: all arm bodies must produce safe results.
            // Exactly one arm executes, analogous to If/Cond.
            HirKind::Match { arms, .. } => arms.iter().all(|(_, _, body)| {
                self.result_is_safe_impl(body, scope_bindings, trust_return_safe)
            }),

            // While always returns nil (an immediate).
            HirKind::While { .. } => true,

            // Loop returns the body's result (non-Recur exit path).
            // Loop bindings are inner scope — add them to scope_bindings
            // so that Var references to loop params are correctly treated
            // as in-scope (not outer).
            HirKind::Loop { bindings, body } => {
                let mut extended: Vec<(Binding, &Hir)> = scope_bindings.to_vec();
                extended.extend(bindings.iter().map(|(b, init)| (*b, init)));
                self.result_is_safe_impl(body, &extended, trust_return_safe)
            }

            // Recur jumps — never produces a result value.
            HirKind::Recur { .. } => true,

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
            HirKind::Parameterize { body, .. } => {
                self.result_is_safe_impl(body, scope_bindings, trust_return_safe)
            }

            // String constants live in the constant pool (LoadConst),
            // not on the fiber heap. Safe to return from a scope.
            HirKind::String(_) => true,

            // Intrinsic: safe iff the op doesn't allocate (arithmetic,
            // comparisons, etc. produce immediates; %list allocates).
            HirKind::Intrinsic { op, .. } => !op.allocates(),

            // Everything else: conservatively unsafe
            // Lambda, Yield, Quote, Eval, Set, Define
            _ => false,
        }
    }

    /// Check if a function call is to a known intrinsic or immediate-returning
    /// primitive/user function, meaning its result is guaranteed to be an immediate.
    /// Check if an argument to a tail call is safe (won't dangle after rotation).
    ///
    /// Like `result_is_safe` but also checks `callee_result_immediate`:
    /// a call to a user function known to return immediates cannot produce
    /// a heap pointer that would dangle when the caller's arena is recycled.
    fn tail_arg_is_safe(&self, hir: &Hir) -> bool {
        // First try the standard result_is_safe check
        if self.result_is_safe(hir, &[]) {
            return true;
        }
        // For Call expressions: check callee_result_immediate
        if let HirKind::Call { func, .. } = &hir.kind {
            let binding = match &func.kind {
                HirKind::Var(b) => Some(b),
                HirKind::DerefCell { cell } => match &cell.kind {
                    HirKind::Var(b) => Some(b),
                    _ => None,
                },
                _ => None,
            };
            if let Some(binding) = binding {
                if self
                    .callee_result_immediate
                    .get(binding)
                    .copied()
                    .unwrap_or(false)
                {
                    return true;
                }
            }
        }
        // Fall back to extended check: recurse into control flow and
        // trust callee_return_safe for Call nodes at any depth.
        self.tail_arg_is_safe_extended(hir)
    }

    /// Extended tail-arg safety check that recurses into control flow
    /// (If, Cond, Begin, And, Or, Let, Letrec, Block, Match, While,
    /// Parameterize) and checks `callee_result_immediate` or
    /// `callee_return_safe` for Call expressions at any depth.
    ///
    /// This handles the nqueens pattern where a tail-call argument is
    /// `(if cond (search ...) count)` — the If wraps a Call that returns
    /// an immediate, but `result_is_safe` can't see through the Call
    /// boundary for letrec-bound callees.
    fn tail_arg_is_safe_extended(&self, hir: &Hir) -> bool {
        match &hir.kind {
            // Literals: always safe
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList
            | HirKind::String(_) => true,

            // Var: safe (pre-existing value, not freshly allocated)
            HirKind::Var(_) => true,

            // Control flow: recurse into all result positions
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.tail_arg_is_safe_extended(then_branch)
                    && self.tail_arg_is_safe_extended(else_branch)
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses
                    .iter()
                    .all(|(_, body)| self.tail_arg_is_safe_extended(body))
                    && else_branch
                        .as_ref()
                        .is_none_or(|b| self.tail_arg_is_safe_extended(b))
            }
            HirKind::Begin(exprs) => exprs
                .last()
                .is_some_and(|e| self.tail_arg_is_safe_extended(e)),
            HirKind::And(exprs) | HirKind::Or(exprs) => {
                exprs.iter().all(|e| self.tail_arg_is_safe_extended(e))
            }
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                self.tail_arg_is_safe_extended(body)
            }
            HirKind::Block { body, .. } => body
                .last()
                .is_some_and(|e| self.tail_arg_is_safe_extended(e)),
            HirKind::Match { arms, .. } => arms
                .iter()
                .all(|(_, _, body)| self.tail_arg_is_safe_extended(body)),
            HirKind::While { .. } | HirKind::Recur { .. } | HirKind::Destructure { .. } => true, // returns nil / jumps
            HirKind::Loop { body, .. } => self.tail_arg_is_safe_extended(body),
            HirKind::Parameterize { body, .. } => self.tail_arg_is_safe_extended(body),

            // Call: check callee_result_immediate OR callee_return_safe
            HirKind::Call { func, args, .. } => {
                // Intrinsics and whitelisted primitives
                if self.call_result_is_safe(func, args) {
                    return true;
                }
                // Look through DerefCell (functionalize wraps letrec bindings)
                let binding = match &func.kind {
                    HirKind::Var(b) => Some(b),
                    HirKind::DerefCell { cell } => match &cell.kind {
                        HirKind::Var(b) => Some(b),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(binding) = binding {
                    if self
                        .callee_result_immediate
                        .get(binding)
                        .copied()
                        .unwrap_or(false)
                    {
                        return true;
                    }
                    if self
                        .callee_return_safe
                        .get(binding)
                        .copied()
                        .unwrap_or(false)
                    {
                        return true;
                    }
                }
                false
            }

            // Intrinsic: non-allocating ops return immediates; %list allocates.
            HirKind::Intrinsic { op, .. } => !op.allocates(),

            // Everything else: conservatively unsafe
            _ => false,
        }
    }

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

        // Check intrinsics map (Conversion with correct arity).
        if let Some(op) = self.intrinsics.get(&sym) {
            return match op {
                IntrinsicOp::Conversion(_) => args.len() == 1,
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
    pub(super) fn callee_is_primitive(&self, func: &Hir) -> bool {
        let binding = match &func.kind {
            HirKind::Var(b) => b,
            HirKind::DerefCell { cell } => match &cell.kind {
                HirKind::Var(b) => b,
                _ => return false,
            },
            _ => return false,
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
                    // AND is a fresh allocation. Non-allocating accessors (rest,
                    // first, get) return pre-existing values that won't be freed
                    // by RegionExit or FlipSwap.
                    if !self.result_is_safe(value, scope_bindings)
                        && !self.value_is_non_allocating_accessor(value)
                    {
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
                        if self.callee_is_arg_escaping_primitive(func) {
                            // Arg-escaping primitives (push, put) insert a
                            // value into a collection. Only the VALUE args
                            // matter: if a value arg is heap-allocated and
                            // scope-local, the scope would free it while the
                            // collection still references it. The target
                            // collection (arg 0) is skipped — a scope-local
                            // target is reclaimed along with its contents.
                            if args
                                .iter()
                                .skip(1)
                                .any(|a| !self.result_is_safe(&a.expr, scope_bindings))
                            {
                                return true;
                            }
                        } else if !self.callee_is_primitive(func)
                            && !self.callee_is_rotation_safe(func)
                            && !self.callee_is_non_escaping_stdlib(func)
                            && !self.callee_is_param_safe(func)
                            && !self.computed_callee_is_safe(func, scope_bindings)
                        {
                            return true;
                        }
                        // Non-escaping primitives (concat, fiber/resume, etc.)
                        // and rotation-safe callees are safe: they consume
                        // args and produce return values, they don't store
                        // args in external mutable structures.
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

            HirKind::Loop { bindings, body } => {
                bindings
                    .iter()
                    .any(|(_, init)| self.walk_for_outward_set(init, scope_bindings))
                    || self.walk_for_outward_set(body, scope_bindings)
            }

            HirKind::Block { body, .. } => body
                .iter()
                .any(|e| self.walk_for_outward_set(e, scope_bindings)),

            HirKind::Break { value, .. } => self.walk_for_outward_set(value, scope_bindings),

            HirKind::Recur { args } => args
                .iter()
                .any(|a| self.walk_for_outward_set(a, scope_bindings)),

            HirKind::Match { value, arms } => {
                self.walk_for_outward_set(value, scope_bindings)
                    || arms.iter().any(|(_, guard, body)| {
                        guard
                            .as_ref()
                            .is_some_and(|g| self.walk_for_outward_set(g, scope_bindings))
                            || self.walk_for_outward_set(body, scope_bindings)
                    })
            }

            HirKind::Emit { value: expr, .. } => self.walk_for_outward_set(expr, scope_bindings),

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

            HirKind::MakeCell { value } => self.walk_for_outward_set(value, scope_bindings),
            HirKind::DerefCell { cell } => self.walk_for_outward_set(cell, scope_bindings),
            HirKind::SetCell { cell, value } => {
                // SetCell writes a value to a CaptureCell/LBox. If the cell
                // references a binding outside the scope and the value is
                // heap-allocated, this is a dangerous outward set — the cell
                // survives RegionExit but the value doesn't.
                let target_in_scope = match &cell.kind {
                    HirKind::Var(target) => scope_bindings.iter().any(|(b, _)| b == target),
                    HirKind::DerefCell { cell: inner } => match &inner.kind {
                        HirKind::Var(target) => scope_bindings.iter().any(|(b, _)| b == target),
                        _ => false,
                    },
                    _ => false,
                };
                if !target_in_scope
                    && !self.result_is_safe(value, scope_bindings)
                    && !self.value_is_non_allocating_accessor(value)
                {
                    return true;
                }
                self.walk_for_outward_set(cell, scope_bindings)
                    || self.walk_for_outward_set(value, scope_bindings)
            }

            // Intrinsics never perform outward set! operations; walk args only.
            HirKind::Intrinsic { args, .. } => args
                .iter()
                .any(|a| self.walk_for_outward_set(a, scope_bindings)),

            HirKind::Error => false,
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

            HirKind::Loop { bindings, body } => {
                bindings
                    .iter()
                    .all(|(_, init)| self.hir_break_values_safe(init, target_id, scope_bindings))
                    && self.hir_break_values_safe(body, target_id, scope_bindings)
            }

            HirKind::Recur { args } => args
                .iter()
                .all(|a| self.hir_break_values_safe(a, target_id, scope_bindings)),

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

            HirKind::Emit { value: expr, .. } => {
                self.hir_break_values_safe(expr, target_id, scope_bindings)
            }

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

            HirKind::MakeCell { value } => {
                self.hir_break_values_safe(value, target_id, scope_bindings)
            }
            HirKind::DerefCell { cell } => {
                self.hir_break_values_safe(cell, target_id, scope_bindings)
            }
            HirKind::SetCell { cell, value } => {
                self.hir_break_values_safe(cell, target_id, scope_bindings)
                    && self.hir_break_values_safe(value, target_id, scope_bindings)
            }

            // Intrinsics contain no breaks; walk args.
            HirKind::Intrinsic { args, .. } => args
                .iter()
                .all(|a| self.hir_break_values_safe(a, target_id, scope_bindings)),

            HirKind::Error => true,
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
    #[allow(dead_code)]
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

            HirKind::Loop { bindings, body } => {
                bindings
                    .iter()
                    .any(|(_, init)| Self::walk_for_escaping_break(init, inner_blocks))
                    || Self::walk_for_escaping_break(body, inner_blocks)
            }

            HirKind::Recur { args } => args
                .iter()
                .any(|a| Self::walk_for_escaping_break(a, inner_blocks)),

            HirKind::Match { value, arms } => {
                Self::walk_for_escaping_break(value, inner_blocks)
                    || arms.iter().any(|(_, guard, body)| {
                        guard
                            .as_ref()
                            .is_some_and(|g| Self::walk_for_escaping_break(g, inner_blocks))
                            || Self::walk_for_escaping_break(body, inner_blocks)
                    })
            }

            HirKind::Emit { value: expr, .. } => Self::walk_for_escaping_break(expr, inner_blocks),

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

            HirKind::MakeCell { value } => Self::walk_for_escaping_break(value, inner_blocks),
            HirKind::DerefCell { cell } => Self::walk_for_escaping_break(cell, inner_blocks),
            HirKind::SetCell { cell, value } => {
                Self::walk_for_escaping_break(cell, inner_blocks)
                    || Self::walk_for_escaping_break(value, inner_blocks)
            }

            // Intrinsics contain no breaks; walk args.
            HirKind::Intrinsic { args, .. } => args
                .iter()
                .any(|a| Self::walk_for_escaping_break(a, inner_blocks)),

            HirKind::Error => false,
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

            HirKind::Loop { bindings, body } => {
                bindings
                    .iter()
                    .all(|(_, init)| self.all_breaks_have_safe_values(init))
                    && self.all_breaks_have_safe_values(body)
            }

            HirKind::Recur { args } => args.iter().all(|a| self.all_breaks_have_safe_values(a)),

            HirKind::Match { value, arms } => {
                self.all_breaks_have_safe_values(value)
                    && arms.iter().all(|(_, guard, body)| {
                        guard
                            .as_ref()
                            .is_none_or(|g| self.all_breaks_have_safe_values(g))
                            && self.all_breaks_have_safe_values(body)
                    })
            }

            HirKind::Emit { value: expr, .. } => self.all_breaks_have_safe_values(expr),

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

            HirKind::MakeCell { value } => self.all_breaks_have_safe_values(value),
            HirKind::DerefCell { cell } => self.all_breaks_have_safe_values(cell),
            HirKind::SetCell { cell, value } => {
                self.all_breaks_have_safe_values(cell) && self.all_breaks_have_safe_values(value)
            }

            // Intrinsics contain no breaks; walk args.
            HirKind::Intrinsic { args, .. } => {
                args.iter().all(|a| self.all_breaks_have_safe_values(a))
            }

            HirKind::Error => true,
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
        HirPattern::Pair { head, tail } => {
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
    /// Check if a HIR subtree references a specific binding.
    ///
    /// Returns true if any `Var(b)` node in the subtree has `b == binding`.
    /// Used by the refined rotation safety check to determine whether a
    /// tail-call argument's expression tree references a specific parameter.
    fn hir_references_binding(hir: &Hir, binding: Binding) -> bool {
        match &hir.kind {
            HirKind::Var(b) => *b == binding,
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList
            | HirKind::String(_)
            | HirKind::Quote(_)
            | HirKind::Error => false,
            // Don't recurse into lambdas — they capture, not reference.
            HirKind::Lambda { .. } => false,
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                Self::hir_references_binding(cond, binding)
                    || Self::hir_references_binding(then_branch, binding)
                    || Self::hir_references_binding(else_branch, binding)
            }
            HirKind::Begin(exprs) | HirKind::And(exprs) | HirKind::Or(exprs) => exprs
                .iter()
                .any(|e| Self::hir_references_binding(e, binding)),
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses.iter().any(|(c, b)| {
                    Self::hir_references_binding(c, binding)
                        || Self::hir_references_binding(b, binding)
                }) || else_branch
                    .as_ref()
                    .is_some_and(|b| Self::hir_references_binding(b, binding))
            }
            HirKind::Call { func, args, .. } => {
                Self::hir_references_binding(func, binding)
                    || args
                        .iter()
                        .any(|a| Self::hir_references_binding(&a.expr, binding))
            }
            HirKind::Assign { value, .. } | HirKind::Define { value, .. } => {
                Self::hir_references_binding(value, binding)
            }
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                bindings
                    .iter()
                    .any(|(_, init)| Self::hir_references_binding(init, binding))
                    || Self::hir_references_binding(body, binding)
            }
            HirKind::While { cond, body } => {
                Self::hir_references_binding(cond, binding)
                    || Self::hir_references_binding(body, binding)
            }
            HirKind::Loop { bindings, body } => {
                bindings
                    .iter()
                    .any(|(_, init)| Self::hir_references_binding(init, binding))
                    || Self::hir_references_binding(body, binding)
            }
            HirKind::Recur { args } => args
                .iter()
                .any(|a| Self::hir_references_binding(a, binding)),
            HirKind::Block { body, .. } => body
                .iter()
                .any(|e| Self::hir_references_binding(e, binding)),
            HirKind::Break { value, .. } => Self::hir_references_binding(value, binding),
            HirKind::Match { value, arms } => {
                Self::hir_references_binding(value, binding)
                    || arms.iter().any(|(_, guard, body)| {
                        guard
                            .as_ref()
                            .is_some_and(|g| Self::hir_references_binding(g, binding))
                            || Self::hir_references_binding(body, binding)
                    })
            }
            HirKind::Emit { value, .. } => Self::hir_references_binding(value, binding),
            HirKind::Destructure { value, .. } => Self::hir_references_binding(value, binding),
            HirKind::Eval { expr, env } => {
                Self::hir_references_binding(expr, binding)
                    || Self::hir_references_binding(env, binding)
            }
            HirKind::Parameterize { bindings, body } => {
                bindings.iter().any(|(p, v)| {
                    Self::hir_references_binding(p, binding)
                        || Self::hir_references_binding(v, binding)
                }) || Self::hir_references_binding(body, binding)
            }
            HirKind::MakeCell { value } => Self::hir_references_binding(value, binding),
            HirKind::DerefCell { cell } => Self::hir_references_binding(cell, binding),
            HirKind::SetCell { cell, value } => {
                Self::hir_references_binding(cell, binding)
                    || Self::hir_references_binding(value, binding)
            }
            // Intrinsics: walk args for binding references.
            HirKind::Intrinsic { args, .. } => args
                .iter()
                .any(|a| Self::hir_references_binding(a, binding)),
        }
    }

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
                //
                // Use the stricter `arg_is_compile_time_immediate` (not
                // `result_is_safe`): for scope/region safety, Vars of
                // outer bindings are "safe" because they were allocated
                // before RegionEnter. But for rotation safety, a Var may
                // hold a heap Value that was freshly allocated in the
                // current rotation frame — pushing it into an external
                // mutable collection creates a dangling reference when
                // rotation drops that frame.
                if self.callee_is_mutating_primitive(func)
                    && args
                        .iter()
                        .any(|a| !Self::arg_is_compile_time_immediate(&a.expr))
                {
                    return true;
                }
                // Tail calls: safe only if no argument is a heap-allocating
                // expression. Rotation recycles the caller's arena, so any
                // heap value passed as an argument would dangle.
                //
                // Use tail_arg_is_safe which extends result_is_safe with
                // callee_result_immediate: a call to a user function known
                // to return immediates cannot produce a dangling heap pointer.
                //
                // Refined for self-tail-calls: per-parameter independence
                // analysis. A heap-allocating arg is safe if it does not
                // reference any parameter whose arg is also heap-allocating.
                // This handles `(loop (- i 1) {:a i :b (pair i nil)})`:
                // arg 1 references param 0 (i), but param 0's arg is
                // immediate, so no cross-generation reference chain.
                if *is_tail {
                    if let (Some(self_binding), Some(ref params)) =
                        (self.current_function_binding, &self.current_function_params)
                    {
                        // Look through DerefCell for letrec-bound callees
                        let callee_binding = match &func.kind {
                            HirKind::Var(b) => Some(b),
                            HirKind::DerefCell { cell } => match &cell.kind {
                                HirKind::Var(b) => Some(b),
                                _ => None,
                            },
                            _ => None,
                        };
                        if let Some(callee_binding) = callee_binding {
                            if *callee_binding == self_binding {
                                // Self-tail-call: per-parameter analysis.
                                let heap_args: Vec<bool> = args
                                    .iter()
                                    .map(|a| !self.tail_arg_is_safe(&a.expr))
                                    .collect();

                                // For each heap-allocating arg, check if it
                                // references any param whose arg is also
                                // heap-allocating. If so, there's a cross-
                                // generation reference chain → unsafe.
                                let escapes = args.iter().enumerate().any(|(k, a)| {
                                    if !heap_args.get(k).copied().unwrap_or(false) {
                                        return false;
                                    }
                                    params.iter().enumerate().any(|(j, param_binding)| {
                                        heap_args.get(j).copied().unwrap_or(false)
                                            && Self::hir_references_binding(&a.expr, *param_binding)
                                    })
                                });
                                return escapes;
                            }
                        }
                    }
                    // Non-self tail call: args must not dangle AND callee
                    // itself must not escape. Without the callee check, a
                    // rotation-safe caller could tail-call a function that
                    // stores its args into external mutable state, creating
                    // a dangling reference when the caller's arena rotates.
                    if args.iter().any(|a| !self.tail_arg_is_safe(&a.expr)) {
                        return true;
                    }
                    if !self.callee_is_primitive(func)
                        && !self.callee_is_rotation_safe(func)
                        && !self.callee_is_non_escaping_stdlib(func)
                    {
                        return true;
                    }
                    return false;
                }
                if !self.callee_is_primitive(func)
                    && !self.callee_is_rotation_safe(func)
                    && !self.callee_is_non_escaping_stdlib(func)
                    && !self.callee_is_param_safe(func)
                {
                    return true;
                }
                self.body_escapes_heap_values(func)
                    || args.iter().any(|a| self.body_escapes_heap_values(&a.expr))
            }
            HirKind::Lambda { .. } => false,
            HirKind::Emit { value, .. } => {
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
            // Loop: recurse into bindings and body (same as Let/While)
            HirKind::Loop { bindings, body } => {
                bindings
                    .iter()
                    .any(|(_, init)| self.body_escapes_heap_values(init))
                    || self.body_escapes_heap_values(body)
            }
            // Recur: args cross iteration boundaries. Only escapes if
            // any arg is heap-allocated (not an immediate/safe value).
            HirKind::Recur { args } => args.iter().any(|a| !self.result_is_safe(a, &[])),
            // Cell ops: structural recursion
            HirKind::MakeCell { value } => self.body_escapes_heap_values(value),
            HirKind::DerefCell { cell } => self.body_escapes_heap_values(cell),
            HirKind::SetCell { cell, value } => {
                self.body_escapes_heap_values(cell) || self.body_escapes_heap_values(value)
            }
            // Intrinsics never store args into external mutable structures;
            // %list stores args into its own freshly-allocated result (not external).
            // Walk args for nested escapes.
            HirKind::Intrinsic { args, .. } => {
                args.iter().any(|a| self.body_escapes_heap_values(a))
            }
            _ => true,
        }
    }

    /// Check if a callee is a known param-safe user function.
    /// A param-safe function never stores its parameters into external
    /// mutable state, so calling it with heap args won't escape them.
    fn callee_is_param_safe(&self, func: &Hir) -> bool {
        let binding = match &func.kind {
            HirKind::Var(b) => b,
            HirKind::DerefCell { cell } => match &cell.kind {
                HirKind::Var(b) => b,
                _ => return self.callee_from_struct_get_is_param_safe(func),
            },
            _ => return self.callee_from_struct_get_is_param_safe(func),
        };
        self.callee_param_safe
            .get(binding)
            .copied()
            .unwrap_or(false)
    }

    /// Like `callee_from_struct_get_is_rotation_safe` but for param-safety.
    fn callee_from_struct_get_is_param_safe(&self, func: &Hir) -> bool {
        let HirKind::Call { args, .. } = &func.kind else {
            return false;
        };
        if !self.value_is_non_allocating_accessor(func) {
            return false;
        }
        let Some(first_arg) = args.first() else {
            return false;
        };
        let struct_binding = match &first_arg.expr.kind {
            HirKind::Var(b) => b,
            HirKind::DerefCell { cell } => match &cell.kind {
                HirKind::Var(b) => b,
                _ => return false,
            },
            _ => return false,
        };
        let bi = self.arena.get(*struct_binding);
        if !bi.is_immutable || bi.is_mutated {
            return false;
        }
        self.callee_struct_fields_param_safe
            .get(struct_binding)
            .copied()
            .unwrap_or(false)
    }

    /// Check whether a function body stores any parameter-derived value
    /// into external mutable state. Used by the param-safety fixpoint.
    ///
    /// `params` is extended as we discover bindings that derive from params
    /// (let init, match destructuring, define).
    pub(super) fn body_stores_params_externally(&self, hir: &Hir, params: &[Binding]) -> bool {
        self.body_stores_params_ext(hir, &mut params.to_vec())
    }

    fn body_stores_params_ext(&self, hir: &Hir, params: &mut Vec<Binding>) -> bool {
        match &hir.kind {
            // Literals, Var, Lambda — no external store
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList
            | HirKind::String(_)
            | HirKind::Quote(_)
            | HirKind::Var(_)
            | HirKind::Error => false,
            // Lambda is a separate scope — params don't flow in
            HirKind::Lambda { .. } => false,

            // Assign to outer binding: escapes if value references a param
            HirKind::Assign { value, .. } => {
                self.expr_references_any_param(value, params)
                    || self.body_stores_params_ext(value, params)
            }
            // SetCell: escapes if value references a param
            HirKind::SetCell { cell, value } => {
                self.expr_references_any_param(value, params)
                    || self.body_stores_params_ext(cell, params)
                    || self.body_stores_params_ext(value, params)
            }
            // Emit: escapes if value references a param
            HirKind::Emit { value, .. } => {
                self.expr_references_any_param(value, params)
                    || self.body_stores_params_ext(value, params)
            }

            // Call
            HirKind::Call { func, args, .. } => {
                // Mutating primitives (push/put): escapes if any arg refs a param
                if self.callee_is_arg_escaping_primitive(func)
                    && args
                        .iter()
                        .any(|a| self.expr_references_any_param(&a.expr, params))
                {
                    return true;
                }
                // Non-primitive, non-param-safe callee: if any arg refs a param, unsafe
                if !self.callee_is_primitive(func)
                    && !self.callee_is_param_safe(func)
                    && !self.callee_is_non_escaping_stdlib(func)
                    && args
                        .iter()
                        .any(|a| self.expr_references_any_param(&a.expr, params))
                {
                    return true;
                }
                // Recurse into func and args for nested stores
                self.body_stores_params_ext(func, params)
                    || args
                        .iter()
                        .any(|a| self.body_stores_params_ext(&a.expr, params))
            }

            // Let/Letrec: check inits, extend param set, recurse body
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (binding, init) in bindings {
                    if self.body_stores_params_ext(init, params) {
                        return true;
                    }
                    // If init references a param, the binding is param-derived
                    if self.expr_references_any_param(init, params) {
                        params.push(*binding);
                    }
                }
                self.body_stores_params_ext(body, params)
            }

            // Define: same logic
            HirKind::Define { binding, value } => {
                if self.body_stores_params_ext(value, params) {
                    return true;
                }
                if self.expr_references_any_param(value, params) {
                    params.push(*binding);
                }
                false
            }

            // Match: if value refs a param, pattern bindings are param-derived
            HirKind::Match { value, arms } => {
                if self.body_stores_params_ext(value, params) {
                    return true;
                }
                let value_refs_param = self.expr_references_any_param(value, params);
                for (pattern, guard, body) in arms {
                    let mut arm_params = params.clone();
                    if value_refs_param {
                        Self::collect_pattern_bindings(pattern, &mut arm_params);
                    }
                    if let Some(g) = guard {
                        if self.body_stores_params_ext(g, &mut arm_params) {
                            return true;
                        }
                    }
                    if self.body_stores_params_ext(body, &mut arm_params) {
                        return true;
                    }
                }
                false
            }

            // Destructure: same as Match
            HirKind::Destructure { pattern, value, .. } => {
                if self.body_stores_params_ext(value, params) {
                    return true;
                }
                if self.expr_references_any_param(value, params) {
                    Self::collect_pattern_bindings(pattern, params);
                }
                false
            }

            // Structural recursion — branches get isolated param snapshots
            // to prevent Define in one branch from contaminating another.
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.body_stores_params_ext(cond, params)
                    || self.body_stores_params_ext(then_branch, &mut params.clone())
                    || self.body_stores_params_ext(else_branch, &mut params.clone())
            }
            HirKind::Begin(exprs) => exprs.iter().any(|e| self.body_stores_params_ext(e, params)),
            HirKind::And(exprs) | HirKind::Or(exprs) => exprs
                .iter()
                .any(|e| self.body_stores_params_ext(e, &mut params.clone())),
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses.iter().any(|(c, b)| {
                    self.body_stores_params_ext(c, &mut params.clone())
                        || self.body_stores_params_ext(b, &mut params.clone())
                }) || else_branch
                    .as_ref()
                    .is_some_and(|b| self.body_stores_params_ext(b, &mut params.clone()))
            }
            HirKind::While { cond, body } => {
                self.body_stores_params_ext(cond, params)
                    || self.body_stores_params_ext(body, params)
            }
            HirKind::Loop { bindings, body } => {
                for (binding, init) in bindings {
                    if self.body_stores_params_ext(init, params) {
                        return true;
                    }
                    if self.expr_references_any_param(init, params) {
                        params.push(*binding);
                    }
                }
                self.body_stores_params_ext(body, params)
            }
            HirKind::Recur { args } => args.iter().any(|a| self.body_stores_params_ext(a, params)),
            HirKind::Block { body, .. } => {
                body.iter().any(|e| self.body_stores_params_ext(e, params))
            }
            HirKind::Break { value, .. } => self.body_stores_params_ext(value, params),
            HirKind::MakeCell { value } => self.body_stores_params_ext(value, params),
            HirKind::DerefCell { cell } => self.body_stores_params_ext(cell, params),
            HirKind::Intrinsic { args, .. } => {
                args.iter().any(|a| self.body_stores_params_ext(a, params))
            }
            HirKind::Parameterize { bindings, body } => {
                bindings
                    .iter()
                    .any(|(_, v)| self.body_stores_params_ext(v, params))
                    || self.body_stores_params_ext(body, params)
            }
            // Eval can execute arbitrary code — conservatively unsafe.
            HirKind::Eval { .. } => true,
        }
    }

    /// Check if an expression references any binding in the param set.
    fn expr_references_any_param(&self, hir: &Hir, params: &[Binding]) -> bool {
        params.iter().any(|p| Self::hir_references_binding(hir, *p))
    }

    /// Collect all Var bindings introduced by a pattern.
    fn collect_pattern_bindings(pattern: &HirPattern, out: &mut Vec<Binding>) {
        match pattern {
            HirPattern::Wildcard | HirPattern::Nil | HirPattern::Literal(_) => {}
            HirPattern::Var(b) => out.push(*b),
            HirPattern::Pair { head, tail } => {
                Self::collect_pattern_bindings(head, out);
                Self::collect_pattern_bindings(tail, out);
            }
            HirPattern::List { elements, rest }
            | HirPattern::Tuple { elements, rest }
            | HirPattern::Array { elements, rest } => {
                for e in elements {
                    Self::collect_pattern_bindings(e, out);
                }
                if let Some(r) = rest {
                    Self::collect_pattern_bindings(r, out);
                }
            }
            HirPattern::Struct { entries, rest } | HirPattern::Table { entries, rest } => {
                for (_, p) in entries {
                    Self::collect_pattern_bindings(p, out);
                }
                if let Some(r) = rest {
                    Self::collect_pattern_bindings(r, out);
                }
            }
            HirPattern::NamedStruct { entries } => {
                for (_, p) in entries {
                    Self::collect_pattern_bindings(p, out);
                }
            }
            HirPattern::Set { binding } | HirPattern::SetMut { binding } => {
                Self::collect_pattern_bindings(binding, out);
            }
            HirPattern::Or(alts) => {
                // All alts bind the same names; just collect from first
                if let Some(first) = alts.first() {
                    Self::collect_pattern_bindings(first, out);
                }
            }
        }
    }

    /// Check if a callee is a known rotation-safe user function.
    /// Uses the `callee_rotation_safe` map populated during lowering.
    /// Looks through DerefCell (functionalize wraps letrec-bound vars).
    fn callee_is_rotation_safe(&self, func: &Hir) -> bool {
        let binding = match &func.kind {
            HirKind::Var(b) => b,
            HirKind::DerefCell { cell } => match &cell.kind {
                HirKind::Var(b) => b,
                _ => return self.callee_from_struct_get_is_rotation_safe(func),
            },
            _ => return self.callee_from_struct_get_is_rotation_safe(func),
        };
        self.callee_rotation_safe
            .get(binding)
            .copied()
            .unwrap_or(false)
    }

    /// Check if a callee expression is `(get struct-var :field)` where the
    /// struct binding holds a struct whose closure fields are all rotation-safe.
    /// This handles the module-init pattern: `(def grace (module-init ...))`
    /// followed by `(grace:bulk-evolve-stream ...)` which desugars to
    /// `((get grace :bulk-evolve-stream) ...)`.
    fn callee_from_struct_get_is_rotation_safe(&self, func: &Hir) -> bool {
        let HirKind::Call { args, .. } = &func.kind else {
            return false;
        };
        // Must be a call to a non-allocating accessor (get, first, rest, etc.)
        if !self.value_is_non_allocating_accessor(func) {
            return false;
        }
        // First arg must be a Var referencing an immutable binding
        let Some(first_arg) = args.first() else {
            return false;
        };
        let struct_binding = match &first_arg.expr.kind {
            HirKind::Var(b) => b,
            HirKind::DerefCell { cell } => match &cell.kind {
                HirKind::Var(b) => b,
                _ => return false,
            },
            _ => return false,
        };
        let bi = self.arena.get(*struct_binding);
        if !bi.is_immutable || bi.is_mutated {
            return false;
        }
        // Check if the struct binding is in the struct-fields-safe map
        self.callee_struct_fields_rotation_safe
            .get(struct_binding)
            .copied()
            .unwrap_or(false)
    }

    fn callee_is_mutating_primitive(&self, func: &Hir) -> bool {
        let binding = match &func.kind {
            HirKind::Var(b) => b,
            HirKind::DerefCell { cell } => match &cell.kind {
                HirKind::Var(b) => b,
                _ => return false,
            },
            _ => return false,
        };
        let bi = self.arena.get(*binding);
        self.mutating_primitives.contains(&bi.name)
    }

    /// Check if the callee is a primitive that stores an argument into a
    /// collection (push, put). These can cause a heap value to escape the
    /// current scope by inserting it into an outer collection.
    fn callee_is_arg_escaping_primitive(&self, func: &Hir) -> bool {
        let HirKind::Var(binding) = &func.kind else {
            return false;
        };
        let bi = self.arena.get(*binding);
        self.arg_escaping_primitives.contains(&bi.name)
    }

    /// QW1: Check if a value is a call to a non-allocating accessor
    /// (rest, first, get, etc.). These return pre-existing values that
    /// were allocated before the current scope/iteration.
    fn value_is_non_allocating_accessor(&self, value: &Hir) -> bool {
        let HirKind::Call { func, .. } = &value.kind else {
            return false;
        };
        let HirKind::Var(binding) = &func.kind else {
            return false;
        };
        let bi = self.arena.get(*binding);
        self.non_allocating_accessors.contains(&bi.name)
    }

    /// QW2: Check if a callee is a known non-escaping stdlib function
    /// (map, filter, etc.). These are pure HOFs that create new
    /// collections without storing args into external structures.
    fn callee_is_non_escaping_stdlib(&self, func: &Hir) -> bool {
        let HirKind::Var(binding) = &func.kind else {
            return false;
        };
        let bi = self.arena.get(*binding);
        self.non_escaping_stdlib.contains(&bi.name)
    }

    /// QW3: Check if a computed (non-Var) callee is safe for outward-set
    /// analysis. Handles `((f))` where f is a rotation-safe lambda that
    /// returns a lambda, and `((fn [] body))` inline anonymous calls.
    fn computed_callee_is_safe(&self, func: &Hir, scope_bindings: &[(Binding, &Hir)]) -> bool {
        // Case 1: func is a Lambda — check its body for outward sets
        if let HirKind::Lambda { body, .. } = &func.kind {
            return !self.walk_for_outward_set(body, scope_bindings);
        }

        // Case 2: func is a Call to a known-safe callee — trace through
        if let HirKind::Call {
            func: inner_func, ..
        } = &func.kind
        {
            if self.callee_is_primitive(inner_func)
                || self.callee_is_rotation_safe(inner_func)
                || self.callee_is_non_escaping_stdlib(inner_func)
            {
                // If the inner callee is a scope-bound lambda, check what
                // it returns. If it returns a Lambda, check that body.
                if let HirKind::Var(binding) = &inner_func.kind {
                    if let Some((_, init)) = scope_bindings.iter().find(|(b, _)| b == binding) {
                        if let HirKind::Lambda { body, .. } = &init.kind {
                            if let HirKind::Lambda {
                                body: inner_body, ..
                            } = &body.kind
                            {
                                return !self.walk_for_outward_set(inner_body, scope_bindings);
                            }
                        }
                    }
                }
                // Inner callee is safe but can't trace return type.
                // Conservatively reject — the result could be a closure
                // that modifies external state when called.
            }
        }

        false
    }

    /// Walk a function body's return positions to check whether all
    /// returned closures are rotation-safe.
    ///
    /// At each return position:
    /// - Lambda: check `!body_escapes_heap_values(body)`
    /// - Call: check `callee_returns_rotation_safe` for the callee
    /// - Var: check `callee_rotation_safe` for the binding
    /// - Control flow: all branches must satisfy
    pub(super) fn body_returns_rotation_safe_closures(&self, hir: &Hir) -> bool {
        match &hir.kind {
            HirKind::Lambda { body, .. } => !self.body_escapes_heap_values(body),
            HirKind::Call { func, .. } => {
                let binding = self.extract_callee_binding(func);
                match binding {
                    Some(b) => self
                        .callee_returns_rotation_safe
                        .get(b)
                        .copied()
                        .unwrap_or(false),
                    None => false,
                }
            }
            HirKind::Var(b) => self.callee_rotation_safe.get(b).copied().unwrap_or(false),
            HirKind::DerefCell { cell } => self.body_returns_rotation_safe_closures(cell),
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.body_returns_rotation_safe_closures(then_branch)
                    && self.body_returns_rotation_safe_closures(else_branch)
            }
            HirKind::Begin(exprs) => exprs
                .last()
                .is_none_or(|e| self.body_returns_rotation_safe_closures(e)),
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                self.body_returns_rotation_safe_closures(body)
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses
                    .iter()
                    .all(|(_, b)| self.body_returns_rotation_safe_closures(b))
                    && else_branch
                        .as_ref()
                        .is_none_or(|b| self.body_returns_rotation_safe_closures(b))
            }
            HirKind::Match { arms, .. } => arms
                .iter()
                .all(|(_, _, body)| self.body_returns_rotation_safe_closures(body)),
            HirKind::Block { body, .. } => body
                .last()
                .is_none_or(|e| self.body_returns_rotation_safe_closures(e)),
            _ => false, // conservative
        }
    }

    /// Walk a function body's return positions to check whether all
    /// returned closures are param-safe.
    pub(super) fn body_returns_param_safe_closures(&self, hir: &Hir) -> bool {
        match &hir.kind {
            HirKind::Lambda { body, params, .. } => {
                !self.body_stores_params_externally(body, params)
            }
            HirKind::Call { func, .. } => {
                let binding = self.extract_callee_binding(func);
                match binding {
                    Some(b) => self
                        .callee_returns_param_safe
                        .get(b)
                        .copied()
                        .unwrap_or(false),
                    None => false,
                }
            }
            HirKind::Var(b) => self.callee_param_safe.get(b).copied().unwrap_or(false),
            HirKind::DerefCell { cell } => self.body_returns_param_safe_closures(cell),
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.body_returns_param_safe_closures(then_branch)
                    && self.body_returns_param_safe_closures(else_branch)
            }
            HirKind::Begin(exprs) => exprs
                .last()
                .is_none_or(|e| self.body_returns_param_safe_closures(e)),
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                self.body_returns_param_safe_closures(body)
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses
                    .iter()
                    .all(|(_, b)| self.body_returns_param_safe_closures(b))
                    && else_branch
                        .as_ref()
                        .is_none_or(|b| self.body_returns_param_safe_closures(b))
            }
            HirKind::Match { arms, .. } => arms
                .iter()
                .all(|(_, _, body)| self.body_returns_param_safe_closures(body)),
            HirKind::Block { body, .. } => body
                .last()
                .is_none_or(|e| self.body_returns_param_safe_closures(e)),
            _ => false, // conservative
        }
    }

    /// Walk a function body's return positions to check whether all
    /// closures in any returned struct are rotation-safe. Like
    /// `body_returns_rotation_safe_closures` but handles struct construction
    /// in return position: all value args of the struct must be rotation-safe
    /// closures (or non-closures).
    pub(super) fn body_returns_struct_fields_rotation_safe(&self, hir: &Hir) -> bool {
        match &hir.kind {
            // Struct construction: check all value args
            HirKind::Call { func, args, .. } if self.callee_is_primitive(func) => {
                // Struct literal: args alternate key, value, key, value...
                // All value args (odd indices) that are closures must be rotation-safe
                args.iter().enumerate().all(|(i, a)| {
                    if i % 2 == 0 {
                        return true; // key position
                    }
                    self.value_is_rotation_safe_closure_or_non_closure(&a.expr)
                })
            }
            // Call to a function: check if it returns struct with safe fields
            HirKind::Call { func, .. } => {
                let binding = self.extract_callee_binding(func);
                match binding {
                    Some(b) => self
                        .callee_struct_fields_rotation_safe
                        .get(b)
                        .copied()
                        .unwrap_or(false),
                    None => false,
                }
            }
            // Control flow: all branches must satisfy
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.body_returns_struct_fields_rotation_safe(then_branch)
                    && self.body_returns_struct_fields_rotation_safe(else_branch)
            }
            HirKind::Begin(exprs) => exprs
                .last()
                .is_none_or(|e| self.body_returns_struct_fields_rotation_safe(e)),
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                self.body_returns_struct_fields_rotation_safe(body)
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses
                    .iter()
                    .all(|(_, b)| self.body_returns_struct_fields_rotation_safe(b))
                    && else_branch
                        .as_ref()
                        .is_none_or(|b| self.body_returns_struct_fields_rotation_safe(b))
            }
            HirKind::Match { arms, .. } => arms
                .iter()
                .all(|(_, _, body)| self.body_returns_struct_fields_rotation_safe(body)),
            HirKind::Block { body, .. } => body
                .last()
                .is_none_or(|e| self.body_returns_struct_fields_rotation_safe(e)),
            _ => false,
        }
    }

    /// Check if an expression is either a rotation-safe closure or not a
    /// closure at all. Used by struct field analysis.
    pub(super) fn value_is_rotation_safe_closure_or_non_closure(&self, hir: &Hir) -> bool {
        match &hir.kind {
            // Lambda: check its body
            HirKind::Lambda { body, .. } => !self.body_escapes_heap_values(body),
            // Var: either a known rotation-safe function or a non-closure value
            HirKind::Var(b) => {
                // If it's in the rotation-safe map, it's a known-safe closure
                if self.callee_rotation_safe.get(b).copied().unwrap_or(false) {
                    return true;
                }
                // If it's NOT in any function map, it's not a closure — safe
                !self.callee_rotation_safe.contains_key(b)
                    && !self.callee_param_safe.contains_key(b)
            }
            HirKind::DerefCell { cell } => self.value_is_rotation_safe_closure_or_non_closure(cell),
            // Literals, keywords, etc. — not closures
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList
            | HirKind::String(_)
            | HirKind::Quote(_) => true,
            _ => false,
        }
    }

    /// Same as `body_returns_struct_fields_rotation_safe` but for param-safety.
    pub(super) fn body_returns_struct_fields_param_safe(&self, hir: &Hir) -> bool {
        match &hir.kind {
            HirKind::Call { func, args, .. } if self.callee_is_primitive(func) => {
                args.iter().enumerate().all(|(i, a)| {
                    if i % 2 == 0 {
                        return true;
                    }
                    self.value_is_param_safe_closure_or_non_closure(&a.expr)
                })
            }
            HirKind::Call { func, .. } => {
                let binding = self.extract_callee_binding(func);
                match binding {
                    Some(b) => self
                        .callee_struct_fields_param_safe
                        .get(b)
                        .copied()
                        .unwrap_or(false),
                    None => false,
                }
            }
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.body_returns_struct_fields_param_safe(then_branch)
                    && self.body_returns_struct_fields_param_safe(else_branch)
            }
            HirKind::Begin(exprs) => exprs
                .last()
                .is_none_or(|e| self.body_returns_struct_fields_param_safe(e)),
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                self.body_returns_struct_fields_param_safe(body)
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses
                    .iter()
                    .all(|(_, b)| self.body_returns_struct_fields_param_safe(b))
                    && else_branch
                        .as_ref()
                        .is_none_or(|b| self.body_returns_struct_fields_param_safe(b))
            }
            HirKind::Match { arms, .. } => arms
                .iter()
                .all(|(_, _, body)| self.body_returns_struct_fields_param_safe(body)),
            HirKind::Block { body, .. } => body
                .last()
                .is_none_or(|e| self.body_returns_struct_fields_param_safe(e)),
            _ => false,
        }
    }

    pub(super) fn value_is_param_safe_closure_or_non_closure(&self, hir: &Hir) -> bool {
        match &hir.kind {
            HirKind::Lambda { body, params, .. } => {
                !self.body_stores_params_externally(body, params)
            }
            HirKind::Var(b) => {
                if self.callee_param_safe.get(b).copied().unwrap_or(false) {
                    return true;
                }
                !self.callee_rotation_safe.contains_key(b)
                    && !self.callee_param_safe.contains_key(b)
            }
            HirKind::DerefCell { cell } => self.value_is_param_safe_closure_or_non_closure(cell),
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList
            | HirKind::String(_)
            | HirKind::Quote(_) => true,
            _ => false,
        }
    }

    /// Resolve the rotation-safety of a non-Lambda init expression.
    /// Returns `Some(true/false)` if resolvable, `None` if dependencies
    /// haven't been resolved yet.
    pub(super) fn resolve_value_rotation_safe(&self, hir: &Hir) -> Option<bool> {
        match &hir.kind {
            HirKind::Lambda { body, .. } => Some(!self.body_escapes_heap_values(body)),
            HirKind::Call { func, .. } => {
                let binding = self.extract_callee_binding(func);
                match binding {
                    Some(b) => self
                        .callee_returns_rotation_safe
                        .get(b)
                        .copied()
                        .map(Some)?,
                    None => Some(false),
                }
            }
            HirKind::Var(b) => self
                .callee_rotation_safe
                .get(b)
                .copied()
                .map(Some)
                .or(Some(None))?,
            HirKind::DerefCell { cell } => self.resolve_value_rotation_safe(cell),
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                let t = self.resolve_value_rotation_safe(then_branch)?;
                let e = self.resolve_value_rotation_safe(else_branch)?;
                Some(t && e)
            }
            HirKind::Begin(exprs) => exprs
                .last()
                .map_or(Some(true), |e| self.resolve_value_rotation_safe(e)),
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                self.resolve_value_rotation_safe(body)
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                let mut all = true;
                for (_, b) in clauses {
                    match self.resolve_value_rotation_safe(b) {
                        Some(v) => all = all && v,
                        None => return None,
                    }
                }
                if let Some(e) = else_branch {
                    match self.resolve_value_rotation_safe(e) {
                        Some(v) => all = all && v,
                        None => return None,
                    }
                }
                Some(all)
            }
            HirKind::Match { arms, .. } => {
                let mut all = true;
                for (_, _, body) in arms {
                    match self.resolve_value_rotation_safe(body) {
                        Some(v) => all = all && v,
                        None => return None,
                    }
                }
                Some(all)
            }
            HirKind::Block { body, .. } => body
                .last()
                .map_or(Some(true), |e| self.resolve_value_rotation_safe(e)),
            _ => None, // can't determine
        }
    }

    /// Resolve the param-safety of a non-Lambda init expression.
    pub(super) fn resolve_value_param_safe(&self, hir: &Hir) -> Option<bool> {
        match &hir.kind {
            HirKind::Lambda { body, params, .. } => {
                Some(!self.body_stores_params_externally(body, params))
            }
            HirKind::Call { func, .. } => {
                let binding = self.extract_callee_binding(func);
                match binding {
                    Some(b) => self.callee_returns_param_safe.get(b).copied().map(Some)?,
                    None => Some(false),
                }
            }
            HirKind::Var(b) => self
                .callee_param_safe
                .get(b)
                .copied()
                .map(Some)
                .or(Some(None))?,
            HirKind::DerefCell { cell } => self.resolve_value_param_safe(cell),
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                let t = self.resolve_value_param_safe(then_branch)?;
                let e = self.resolve_value_param_safe(else_branch)?;
                Some(t && e)
            }
            HirKind::Begin(exprs) => exprs
                .last()
                .map_or(Some(true), |e| self.resolve_value_param_safe(e)),
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                self.resolve_value_param_safe(body)
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                let mut all = true;
                for (_, b) in clauses {
                    match self.resolve_value_param_safe(b) {
                        Some(v) => all = all && v,
                        None => return None,
                    }
                }
                if let Some(e) = else_branch {
                    match self.resolve_value_param_safe(e) {
                        Some(v) => all = all && v,
                        None => return None,
                    }
                }
                Some(all)
            }
            HirKind::Match { arms, .. } => {
                let mut all = true;
                for (_, _, body) in arms {
                    match self.resolve_value_param_safe(body) {
                        Some(v) => all = all && v,
                        None => return None,
                    }
                }
                Some(all)
            }
            HirKind::Block { body, .. } => body
                .last()
                .map_or(Some(true), |e| self.resolve_value_param_safe(e)),
            _ => None, // can't determine
        }
    }

    /// Extract the Binding from a callee expression (Var or DerefCell{Var}).
    pub(super) fn extract_callee_binding<'h>(&self, func: &'h Hir) -> Option<&'h Binding> {
        match &func.kind {
            HirKind::Var(b) => Some(b),
            HirKind::DerefCell { cell } => match &cell.kind {
                HirKind::Var(b) => Some(b),
                _ => None,
            },
            _ => None,
        }
    }

    /// Check if an expression is statically proven to produce an immediate
    /// (non-heap) Value. Conservative: returns false if we cannot prove it.
    ///
    /// This is stricter than `result_is_safe`, which is about scope/region
    /// lifetimes (outer-bound Vars are "safe" because they outlive the
    /// scope). For rotation-safety analysis we need value-type certainty:
    /// a Var may hold a heap Value regardless of its scope, so mutating-
    /// primitive calls with Var args must be treated as escaping.
    fn arg_is_compile_time_immediate(hir: &Hir) -> bool {
        match &hir.kind {
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList
            // String literals live in the constant pool, not the fiber heap.
            | HirKind::String(_) => true,
            _ => false,
        }
    }

    /// Determine whether a while loop's body is safe for FlipSwap injection.
    ///
    /// Two conditions must hold:
    /// 1. No dangerous outward set — body doesn't write heap values to
    ///    bindings defined outside the loop.
    /// 2. All break values are safe immediates — breaks don't carry heap
    ///    pointers past FlipExit.
    ///
    /// Suspension is NOT a rejection condition. The runtime's
    /// `rotate_pools` returns early when `shared_alloc` is non-null,
    /// so FlipSwap on shared-alloc fibers is a safe no-op.
    pub(super) fn can_flip_while_loop(
        &self,
        body: &Hir,
        loop_bindings: &[(Binding, &Hir)],
    ) -> bool {
        if self.body_contains_dangerous_outward_set(body, loop_bindings) {
            return false;
        }
        if !self.all_breaks_have_safe_values(body) {
            return false;
        }
        true
    }

    /// Like `can_flip_while_loop` but relaxed for deferred refcounting.
    ///
    /// With refcounting, outward mutations via put/push to mutable
    /// collections are safe: the old value gets decref'd and the new
    /// value gets incref'd, so pinned values survive scope exit.
    ///
    /// Eligible when:
    /// - Break values are safe (same as flip)
    /// - No dangerous outward assigns (not refcounted)
    /// - The ONLY reason `can_flip_while_loop` rejected is arg-escaping
    ///   primitives (put/push), not other unsafe patterns
    pub(super) fn can_flip_while_loop_refcounted(
        &self,
        body: &Hir,
        loop_bindings: &[(Binding, &Hir)],
    ) -> bool {
        // Loop body must not suspend: suspended fibers hold references to
        // loop-scope objects on their stack. Refcounted rotation at the
        // back-edge would free those objects (rc=0, not in any collection),
        // causing use-after-free when the fiber resumes.
        if body.signal.may_suspend() {
            return false;
        }
        if !self.all_breaks_have_safe_values(body) {
            return false;
        }
        // The body must not have dangerous outward assigns.
        if self.body_contains_dangerous_outward_assign(body, loop_bindings) {
            return false;
        }
        // The body MUST have an arg-escaping primitive (put/push) that
        // blocked flip eligibility — otherwise flip would have accepted.
        // This prevents enabling refcounted scope marks for patterns
        // we haven't analyzed.
        self.body_has_arg_escaping_call(body, loop_bindings)
    }

    /// Check if the body contains at least one call to an arg-escaping
    /// primitive (put/push) with a non-safe value arg directed at an
    /// outer collection. This is the specific pattern refcounting fixes.
    fn body_has_arg_escaping_call(&self, hir: &Hir, scope_bindings: &[(Binding, &Hir)]) -> bool {
        match &hir.kind {
            HirKind::Lambda { .. } => false,
            HirKind::Call {
                func,
                args,
                is_tail,
            } => {
                if !*is_tail
                    && self.callee_is_arg_escaping_primitive(func)
                    && args
                        .iter()
                        .skip(1)
                        .any(|a| !self.result_is_safe(&a.expr, scope_bindings))
                {
                    return true;
                }
                // Recurse into subexpressions
                let mut found = false;
                hir.for_each_child(|child| {
                    if !found && self.body_has_arg_escaping_call(child, scope_bindings) {
                        found = true;
                    }
                });
                found
            }
            _ => {
                let mut found = false;
                hir.for_each_child(|child| {
                    if !found && self.body_has_arg_escaping_call(child, scope_bindings) {
                        found = true;
                    }
                });
                found
            }
        }
    }

    /// Like `body_contains_dangerous_outward_set` but only checks for
    /// `assign` to outer bindings, not calls to arg-escaping primitives
    /// (put/push). Those are handled by refcounting.
    fn body_contains_dangerous_outward_assign(
        &self,
        hir: &Hir,
        scope_bindings: &[(Binding, &Hir)],
    ) -> bool {
        match &hir.kind {
            HirKind::Assign { target, value } => {
                let in_scope = scope_bindings.iter().any(|(b, _)| b == target);
                if !in_scope
                    && !self.result_is_safe(value, scope_bindings)
                    && !self.value_is_non_allocating_accessor(value)
                {
                    return true;
                }
                self.body_contains_dangerous_outward_assign(value, scope_bindings)
            }
            HirKind::SetCell { cell, value } => {
                // SetCell is the functionalized form of Assign. Check if the
                // cell target is outside scope — same logic as Assign.
                let target_in_scope = match &cell.kind {
                    HirKind::Var(target) => scope_bindings.iter().any(|(b, _)| b == target),
                    HirKind::DerefCell { cell: inner } => match &inner.kind {
                        HirKind::Var(target) => scope_bindings.iter().any(|(b, _)| b == target),
                        _ => false,
                    },
                    _ => false,
                };
                if !target_in_scope
                    && !self.result_is_safe(value, scope_bindings)
                    && !self.value_is_non_allocating_accessor(value)
                {
                    return true;
                }
                self.body_contains_dangerous_outward_assign(cell, scope_bindings)
                    || self.body_contains_dangerous_outward_assign(value, scope_bindings)
            }
            HirKind::Lambda { .. } => false,
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList
            | HirKind::String(_)
            | HirKind::Var(_) => false,
            _ => {
                // Recurse into all children
                let mut found = false;
                hir.for_each_child(|child| {
                    if !found && self.body_contains_dangerous_outward_assign(child, scope_bindings)
                    {
                        found = true;
                    }
                });
                found
            }
        }
    }

    /// Check if dealloc_slot is safe within RegionRotate for a loop body.
    ///
    /// Double-buffered scope marks delay freeing by one iteration, so
    /// recur arg values survive the rotation. But if a loop param is
    /// assigned a heap value that REFERENCES the previous param value
    /// (e.g., `(pair val acc)` where cdr → old acc), freeing iteration
    /// N-1 corrupts iteration N's cons chain.
    ///
    /// Returns false when a loop param is assigned a non-safe value whose
    /// arguments include another loop param — indicating a reference chain
    /// across iterations.
    pub(super) fn can_dealloc_in_loop(
        &self,
        body: &Hir,
        loop_bindings: &[(Binding, &Hir)],
    ) -> bool {
        !self.loop_param_chains(body, loop_bindings)
    }

    /// Check if a loop param is assigned a value that chains across iterations.
    ///
    /// True when: `(assign loop_param (call ... other_loop_param ...))` — the
    /// call's result may contain a reference to the old param value.
    fn loop_param_chains(&self, hir: &Hir, scope_bindings: &[(Binding, &Hir)]) -> bool {
        match &hir.kind {
            HirKind::Assign { target, value } => {
                let is_loop_param = scope_bindings.iter().any(|(b, _)| b == target);
                if is_loop_param
                    && !self.result_is_safe(value, scope_bindings)
                    && Self::expr_references_loop_param(value, scope_bindings)
                {
                    return true;
                }
                self.loop_param_chains(value, scope_bindings)
            }
            HirKind::Lambda { .. } | HirKind::Loop { .. } => false,
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.loop_param_chains(cond, scope_bindings)
                    || self.loop_param_chains(then_branch, scope_bindings)
                    || self.loop_param_chains(else_branch, scope_bindings)
            }
            HirKind::Begin(exprs) | HirKind::And(exprs) | HirKind::Or(exprs) => exprs
                .iter()
                .any(|e| self.loop_param_chains(e, scope_bindings)),
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                bindings
                    .iter()
                    .any(|(_, init)| self.loop_param_chains(init, scope_bindings))
                    || self.loop_param_chains(body, scope_bindings)
            }
            HirKind::Call { func, args, .. } => {
                self.loop_param_chains(func, scope_bindings)
                    || args
                        .iter()
                        .any(|a| self.loop_param_chains(&a.expr, scope_bindings))
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses.iter().any(|(c, b)| {
                    self.loop_param_chains(c, scope_bindings)
                        || self.loop_param_chains(b, scope_bindings)
                }) || else_branch
                    .as_ref()
                    .is_some_and(|b| self.loop_param_chains(b, scope_bindings))
            }
            HirKind::Match { value, arms } => {
                self.loop_param_chains(value, scope_bindings)
                    || arms.iter().any(|(_, guard, body)| {
                        guard
                            .as_ref()
                            .is_some_and(|g| self.loop_param_chains(g, scope_bindings))
                            || self.loop_param_chains(body, scope_bindings)
                    })
            }
            HirKind::Block { body, .. } => body
                .iter()
                .any(|e| self.loop_param_chains(e, scope_bindings)),
            HirKind::While { cond, body } => {
                self.loop_param_chains(cond, scope_bindings)
                    || self.loop_param_chains(body, scope_bindings)
            }
            HirKind::SetCell { cell, value } => {
                self.loop_param_chains(cell, scope_bindings)
                    || self.loop_param_chains(value, scope_bindings)
            }
            HirKind::MakeCell { value }
            | HirKind::DerefCell { cell: value }
            | HirKind::Define { value, .. }
            | HirKind::Break { value, .. }
            | HirKind::Emit { value, .. } => self.loop_param_chains(value, scope_bindings),
            HirKind::Recur { args } => args
                .iter()
                .any(|a| self.loop_param_chains(a, scope_bindings)),
            HirKind::Intrinsic { args, .. } => args
                .iter()
                .any(|a| self.loop_param_chains(a, scope_bindings)),
            HirKind::Eval { expr, env } => {
                self.loop_param_chains(expr, scope_bindings)
                    || self.loop_param_chains(env, scope_bindings)
            }
            HirKind::Parameterize { bindings, body } => {
                bindings.iter().any(|(k, v)| {
                    self.loop_param_chains(k, scope_bindings)
                        || self.loop_param_chains(v, scope_bindings)
                }) || self.loop_param_chains(body, scope_bindings)
            }
            HirKind::Destructure { value, .. } => self.loop_param_chains(value, scope_bindings),
            _ => false,
        }
    }

    /// Check if an expression directly references a loop parameter Var.
    fn expr_references_loop_param(hir: &Hir, scope_bindings: &[(Binding, &Hir)]) -> bool {
        match &hir.kind {
            HirKind::Var(b) => scope_bindings.iter().any(|(sb, _)| sb == b),
            HirKind::Call { func, args, .. } => {
                Self::expr_references_loop_param(func, scope_bindings)
                    || args
                        .iter()
                        .any(|a| Self::expr_references_loop_param(&a.expr, scope_bindings))
            }
            HirKind::Intrinsic { args, .. } => args
                .iter()
                .any(|a| Self::expr_references_loop_param(a, scope_bindings)),
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                Self::expr_references_loop_param(cond, scope_bindings)
                    || Self::expr_references_loop_param(then_branch, scope_bindings)
                    || Self::expr_references_loop_param(else_branch, scope_bindings)
            }
            HirKind::Begin(exprs) => exprs
                .iter()
                .any(|e| Self::expr_references_loop_param(e, scope_bindings)),
            HirKind::DerefCell { cell } => Self::expr_references_loop_param(cell, scope_bindings),
            // Don't recurse into lambdas
            HirKind::Lambda { .. } => false,
            _ => false,
        }
    }
}
