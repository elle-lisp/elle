//! HIR to LIR lowering

mod access;
mod binding;
mod control;
pub(crate) mod decision;
mod escape;
mod expr;
mod lambda;
mod pattern;

use super::intrinsics::IntrinsicOp;
use super::types::*;
use crate::hir::arena::BindingArena;
use crate::hir::{Binding, BlockId, CallArg, Hir, HirKind, HirPattern};
use crate::syntax::Span;
use crate::value::fiber::SignalBits;
use crate::value::{Arity, SymbolId, Value};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::HashMap;
use std::fmt;

/// Compile-time scope allocation statistics.
///
/// Tracks how many let/letrec/block scopes were analyzed for scope
/// allocation, how many qualified, and why the rest were rejected.
/// The rejection reason is the *first* failing condition (conditions
/// are checked in order and short-circuit).
#[derive(Debug, Clone, Default)]
pub struct ScopeStats {
    /// Total scopes evaluated for scope allocation
    pub scopes_analyzed: usize,
    /// Scopes that passed all conditions (RegionEnter/RegionExit emitted)
    pub scopes_qualified: usize,
    /// Scopes rejected because a binding is captured (condition 1)
    pub rejected_captured: usize,
    /// Scopes rejected because body may suspend (condition 2)
    pub rejected_suspends: usize,
    /// Scopes rejected because result is not provably immediate (condition 3)
    pub rejected_unsafe_result: usize,
    /// Scopes rejected because body contains set to outer binding (condition 4)
    pub rejected_outward_set: usize,
    /// Scopes rejected because body contains break (condition 5)
    pub rejected_break: usize,
    /// Non-tail calls wrapped in RegionEnter/RegionExit
    pub calls_scoped: usize,
}

impl ScopeStats {
    /// Total rejected scopes (analyzed - qualified).
    pub fn scopes_rejected(&self) -> usize {
        self.scopes_analyzed - self.scopes_qualified
    }

    /// Merge another ScopeStats into this one (for aggregating across lowerer invocations).
    pub fn merge(&mut self, other: &ScopeStats) {
        self.scopes_analyzed += other.scopes_analyzed;
        self.scopes_qualified += other.scopes_qualified;
        self.rejected_captured += other.rejected_captured;
        self.rejected_suspends += other.rejected_suspends;
        self.rejected_unsafe_result += other.rejected_unsafe_result;
        self.rejected_outward_set += other.rejected_outward_set;
        self.rejected_break += other.rejected_break;
        self.calls_scoped += other.calls_scoped;
    }
}

impl fmt::Display for ScopeStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "scope allocation stats:")?;
        writeln!(
            f,
            "  analyzed: {}  qualified: {}  rejected: {}",
            self.scopes_analyzed,
            self.scopes_qualified,
            self.scopes_rejected()
        )?;
        if self.scopes_rejected() > 0 {
            writeln!(f, "  rejection reasons:")?;
            if self.rejected_captured > 0 {
                writeln!(f, "    captured:      {}", self.rejected_captured)?;
            }
            if self.rejected_suspends > 0 {
                writeln!(f, "    suspends:      {}", self.rejected_suspends)?;
            }
            if self.rejected_unsafe_result > 0 {
                writeln!(f, "    unsafe-result: {}", self.rejected_unsafe_result)?;
            }
            if self.rejected_outward_set > 0 {
                writeln!(f, "    outward-set:   {}", self.rejected_outward_set)?;
            }
            if self.rejected_break > 0 {
                writeln!(f, "    break:         {}", self.rejected_break)?;
            }
        }
        if self.calls_scoped > 0 {
            writeln!(f, "  call-scoped:   {}", self.calls_scoped)?;
        }
        Ok(())
    }
}

/// Tracks an active block during lowering so `break` can find its
/// result register and exit label.
struct BlockLowerContext {
    block_id: BlockId,
    #[allow(dead_code)]
    result_reg: Reg,
    result_slot: u16,
    exit_label: Label,
    /// The `region_depth` at the time this block was entered.
    /// `break` emits `(current_region_depth - region_depth_at_entry)`
    /// compensating `RegionExit` instructions before jumping to the exit.
    region_depth_at_entry: u32,
}

/// Lowers HIR to LIR
pub struct Lowerer<'a> {
    arena: &'a BindingArena,
    /// Current function being built
    current_func: LirFunction,
    /// Current block being built
    current_block: BasicBlock,
    /// Next register ID
    next_reg: u32,
    /// Next label ID
    next_label: u32,
    /// Mapping from Binding to local slot
    binding_to_slot: HashMap<Binding, u16>,
    /// Whether we're currently lowering a lambda (closure)
    in_lambda: bool,
    /// Number of captured variables (for lambda context)
    num_captures: u16,
    /// Number of parameters allocated as locals (non-LBox, non-captured params).
    /// Used by allocate_slot to compute lbox_locals_mask offsets.
    num_local_params: u16,
    /// Set of bindings that are upvalues (captures/parameters in lambda)
    /// These use LoadCapture/StoreCapture, not LoadLocal/StoreLocal
    upvalue_bindings: std::collections::HashSet<Binding>,
    /// Current span for emitted instructions
    current_span: Span,
    /// Intrinsic operations for operator specialization.
    /// Maps global SymbolId to specialized LIR instruction.
    intrinsics: FxHashMap<SymbolId, IntrinsicOp>,
    /// Primitives known to return immediates.
    /// Used by escape analysis (`result_is_safe`) to accept calls to
    /// these primitives in scope-allocated let bodies.
    immediate_primitives: FxHashSet<SymbolId>,
    mutating_primitives: FxHashSet<SymbolId>,
    /// Binding → rotation_safe for lowered lambdas. Populated during
    /// lowering so that `body_escapes_heap_values` can check callees
    /// transitively: a call to a rotation-safe function doesn't escape.
    callee_rotation_safe: HashMap<Binding, bool>,
    /// Binding → result_is_immediate for function definitions.
    /// Precomputed via fixpoint iteration so that `call_result_is_safe`
    /// can identify user functions that always return immediates.
    callee_result_immediate: HashMap<Binding, bool>,
    /// Compile-time constant values for immutable bindings (for LoadConst optimization)
    immutable_values: HashMap<Binding, Value>,
    /// Stack of active block contexts for `break` lowering
    block_lower_contexts: Vec<BlockLowerContext>,
    /// Current nesting depth of active allocation regions.
    /// Incremented on `RegionEnter`, decremented on `RegionExit`.
    /// Used by `lower_break` to emit compensating `RegionExit`s.
    region_depth: u32,
    /// Compile-time scope allocation statistics.
    scope_stats: ScopeStats,
    /// Scratch slot for discarding unused intermediate values.
    /// Lazily allocated on first use. Reused across all discards
    /// within the same function, so only one extra local slot.
    discard_slot: Option<u16>,
    /// Symbol ID → name mapping for error messages.
    symbol_names: HashMap<u32, String>,
    /// Count of pending RegionExit instructions that TailCall emissions
    /// must emit before replacing the frame. Incremented by `lower_let`
    /// / `lower_letrec` when the body is a tail call and the scope is
    /// allocated. Decremented after the body is lowered.
    ///
    /// Uses a counter (not a bool) because `if` branches are lowered
    /// sequentially against the same lowerer state — a bool consumed by
    /// the first branch's tail call leaves the second branch without
    /// RegionExit. The counter stays constant across branches so every
    /// tail-call site emits the correct number of exits.
    pending_region_exits: u32,
}

impl<'a> Lowerer<'a> {
    pub fn new(arena: &'a BindingArena) -> Self {
        Lowerer {
            arena,
            current_func: LirFunction::new(Arity::Exact(0)),
            current_block: BasicBlock::new(Label(0)),
            next_reg: 0,
            next_label: 1, // 0 is entry
            binding_to_slot: HashMap::new(),
            in_lambda: false,
            num_captures: 0,
            num_local_params: 0,
            upvalue_bindings: std::collections::HashSet::new(),
            current_span: Span::synthetic(),
            intrinsics: FxHashMap::default(),
            immediate_primitives: FxHashSet::default(),
            mutating_primitives: FxHashSet::default(),
            callee_rotation_safe: HashMap::new(),
            callee_result_immediate: HashMap::new(),
            immutable_values: HashMap::new(),
            block_lower_contexts: Vec::new(),
            region_depth: 0,
            scope_stats: ScopeStats::default(),
            discard_slot: None,
            symbol_names: HashMap::new(),
            pending_region_exits: 0,
        }
    }

    /// Set intrinsic operations for operator specialization
    pub(crate) fn with_intrinsics(mut self, intrinsics: FxHashMap<SymbolId, IntrinsicOp>) -> Self {
        self.intrinsics = intrinsics;
        self
    }

    /// Set the whitelist of primitives known to return immediates
    pub fn with_immediate_primitives(mut self, set: FxHashSet<SymbolId>) -> Self {
        self.immediate_primitives = set;
        self
    }

    pub fn with_mutating_primitives(mut self, set: FxHashSet<SymbolId>) -> Self {
        self.mutating_primitives = set;
        self
    }

    /// Set symbol names for error messages.
    pub fn with_symbol_names(mut self, names: HashMap<u32, String>) -> Self {
        self.symbol_names = names;
        self
    }

    /// Seed `immutable_values` with primitive binding→value pairs.
    ///
    /// Primitive bindings are `BindingScope::Local` with `mark_immutable()`.
    /// The lowerer never allocates slots for them — instead, `lower_var`
    /// checks `immutable_values` first and emits `LoadConst` for any
    /// binding with a known constant value.
    pub fn with_primitive_values(mut self, values: HashMap<Binding, Value>) -> Self {
        self.immutable_values.extend(values);
        self
    }

    /// Return compile-time scope allocation statistics.
    pub fn scope_stats(&self) -> &ScopeStats {
        &self.scope_stats
    }

    /// Lower a HIR expression to LIR
    pub fn lower(&mut self, hir: &Hir) -> Result<LirFunction, String> {
        // Precompute interprocedural properties for all function definitions
        // in the compilation unit. Uses fixpoint iteration to handle mutual
        // recursion (e.g. try-col ↔ search in nqueens).
        //
        // Order matters: result_immediate depends on call_result_is_safe
        // which checks callee_result_immediate; rotation_safety depends on
        // body_escapes_heap_values which checks callee_rotation_safe.
        // Both converge independently.
        self.precompute_result_immediate(hir);
        self.precompute_rotation_safety(hir);

        self.current_func = LirFunction::new(Arity::Exact(0));
        self.current_block = BasicBlock::new(Label(0));
        self.next_reg = 0;
        self.next_label = 1;
        self.binding_to_slot.clear();
        self.discard_slot = None;

        let result_reg = self.lower_expr(hir)?;
        self.terminate(Terminator::Return(result_reg));
        self.finish_block();

        self.current_func.entry = Label(0);
        self.current_func.num_regs = self.next_reg;
        // Propagate signal from HIR to top-level LIR function
        self.current_func.signal = hir.signal;

        // Compute escape analysis flags for fiber shared-alloc decisions.
        // Covers closures created from top-level `defn` forms passed to
        // `fiber/new` as variables.
        self.current_func.result_is_immediate = self.result_is_safe(hir, &[]);
        self.current_func.has_outward_heap_set = self.body_contains_dangerous_outward_set(hir, &[]);
        self.current_func.rotation_safe = !self.body_escapes_heap_values(hir);

        Ok(std::mem::replace(
            &mut self.current_func,
            LirFunction::new(Arity::Exact(0)),
        ))
    }

    // === Helper Methods ===

    fn fresh_reg(&mut self) -> Reg {
        let r = Reg::new(self.next_reg);
        self.next_reg += 1;
        r
    }

    fn allocate_slot(&mut self, binding: Binding) -> u16 {
        // Inside a lambda, two address spaces coexist:
        //   - Env (captures + params + LBox locals): LoadCapture/StoreCapture
        //   - Stack/register locals (non-LBox let-bound): LoadLocal/StoreLocal
        //
        // Environment layout: [captures..., params..., lbox_locals..., nil_placeholders...]
        // Stack frame layout:  [params..., all_locally_defined...]
        //
        // LBox locals get ENV-relative slots (num_captures + num_locals).
        // Non-LBox locals get STACK-relative slots (num_locals).
        // Both increment num_locals to keep env placeholder slots aligned.
        let needs_lbox = self.arena.get(binding).needs_lbox();
        let slot = if self.in_lambda {
            // local_index is relative to locally-defined vars (after param locals)
            let local_index = self.current_func.num_locals - self.num_local_params;
            if needs_lbox && local_index < 64 {
                self.current_func.lbox_locals_mask |= 1 << local_index;
            }
            if needs_lbox {
                // Env-relative: for LoadCapture/StoreCapture
                self.num_captures + self.current_func.num_locals
            } else {
                // Stack-relative: for LoadLocal/StoreLocal
                self.current_func.num_locals
            }
        } else {
            self.current_func.num_locals
        };
        self.current_func.num_locals += 1;
        self.binding_to_slot.insert(binding, slot);
        slot
    }

    fn emit(&mut self, instr: LirInstr) {
        self.current_block
            .instructions
            .push(SpannedInstr::new(instr, self.current_span.clone()));
    }

    fn emit_const(&mut self, c: LirConst) -> Result<Reg, String> {
        let dst = self.fresh_reg();
        self.emit(LirInstr::Const { dst, value: c });
        Ok(dst)
    }

    fn emit_value_const(&mut self, value: Value) -> Result<Reg, String> {
        let dst = self.fresh_reg();
        self.emit(LirInstr::ValueConst { dst, value });
        Ok(dst)
    }

    fn terminate(&mut self, term: Terminator) {
        self.current_block.terminator = SpannedTerminator::new(term, self.current_span.clone());
    }

    fn finish_block(&mut self) {
        let block = std::mem::replace(&mut self.current_block, BasicBlock::new(Label(0)));
        self.current_func.blocks.push(block);
    }

    /// Allocate a new basic block label.
    fn fresh_label(&mut self) -> Label {
        let label = Label(self.next_label);
        self.next_label += 1;
        label
    }

    /// Finish the current block and start a new one with the given label.
    fn start_new_block(&mut self, label: Label) {
        self.finish_block();
        self.current_block = BasicBlock::new(label);
    }

    /// Emit `RegionEnter` and increment the region depth counter.
    fn emit_region_enter(&mut self) {
        self.emit(LirInstr::RegionEnter);
        self.region_depth += 1;
    }

    /// Emit `RegionExit` and decrement the region depth counter.
    fn emit_region_exit(&mut self) {
        self.emit(LirInstr::RegionExit);
        self.region_depth -= 1;
    }

    /// Discard an unused value by storing it to a scratch slot.
    /// The emitter's auto-pop after StoreLocal cleans up the value
    /// from the operand stack. The scratch slot is lazily allocated
    /// on first use and reused for all discards in the function.
    fn discard(&mut self, src: Reg) {
        let slot = match self.discard_slot {
            Some(s) => s,
            None => {
                let s = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.discard_slot = Some(s);
                s
            }
        };
        self.emit(LirInstr::StoreLocal { slot, src });
    }

    // ── Escape analysis ────────────────────────────────────────────
    //
    // See `escape.rs` for helper functions (`result_is_safe`,
    // `body_contains_dangerous_outward_set`, `body_contains_escaping_break`,
    // `all_break_values_safe`, `all_breaks_have_safe_values`).

    /// Determine if a `let` scope's allocations can be safely released
    /// at scope exit via `RegionEnter`/`RegionExit`.
    ///
    /// Performs escape analysis on the let body to check if all bindings
    /// and intermediate values allocated within the scope can be freed
    /// when the scope exits. This enables the lowerer to emit `RegionEnter`
    /// and `RegionExit` instructions for automatic cleanup.
    ///
    /// Returns `true` when ALL six conditions hold:
    /// 1. No binding is captured by a nested lambda (captured values escape)
    /// 2. Body cannot suspend (yield/debug/polymorphic signals prevent cleanup)
    /// 3. Body result is provably an immediate (not heap-allocated)
    /// 4. Body contains no dangerous outward `set` (set to outer binding
    ///    with a value that could be heap-allocated inside the scope)
    /// 5. All breaks in body carry safe immediate values
    /// 6. Body contains no `break` targeting outer blocks (break carries
    ///    a value past RegionExit, causing use-after-free)
    ///
    /// Increments `scope_stats.scopes_analyzed` and updates rejection counters
    /// for each failed condition (short-circuits on first failure).
    fn can_scope_allocate_let(&mut self, bindings: &[(Binding, Hir)], body: &Hir) -> bool {
        self.scope_stats.scopes_analyzed += 1;
        // Condition 1: no captures
        if bindings.iter().any(|(b, _)| self.arena.get(*b).is_captured) {
            self.scope_stats.rejected_captured += 1;
            return false;
        }

        // Condition 2: no suspension in body or binding inits.
        // Binding init expressions are evaluated inside the region, so
        // allocations made by callees during a binding init are freed by
        // RegionExit. If a binding init suspends, the caller's body may
        // later create heap objects that escape via side effects (e.g. put
        // to an external mutable struct), and RegionExit would free them
        // while they're still referenced externally.
        //
        // Exception: when the body is a PURE tail call (no preceding
        // expressions that could suspend), the tail call's signal doesn't
        // matter — RegionExit fires before the tail call executes.
        // But if the body has non-tail sub-expressions that may suspend
        // (e.g. `(begin (port/write p x) (tail-call))`), those expressions
        // run within the scope and suspension is still dangerous.
        let body_suspends = if Self::body_is_tail_call(body) {
            Self::non_tail_subexprs_may_suspend(body)
        } else {
            body.signal.may_suspend()
        };
        if body_suspends || bindings.iter().any(|(_, init)| init.signal.may_suspend()) {
            self.scope_stats.rejected_suspends += 1;
            return false;
        }

        // Build scope binding refs once — used by conditions 3 and 4
        let scope_binding_refs: Vec<(Binding, &Hir)> =
            bindings.iter().map(|(b, init)| (*b, init)).collect();

        // Condition 3: result is immediate
        if !self.result_is_safe(body, &scope_binding_refs) {
            self.scope_stats.rejected_unsafe_result += 1;
            return false;
        }

        // Condition 4: no dangerous outward mutation
        if self.body_contains_dangerous_outward_set(body, &scope_binding_refs) {
            self.scope_stats.rejected_outward_set += 1;
            return false;
        }

        // Condition 5: all breaks carry safe immediate values.
        if !self.all_breaks_have_safe_values(body) {
            self.scope_stats.rejected_break += 1;
            return false;
        }

        // Condition 6: no escaping break.
        if Self::hir_contains_escaping_break(body) {
            self.scope_stats.rejected_break += 1;
            return false;
        }

        self.scope_stats.scopes_qualified += 1;
        true
    }

    /// Determine if a `letrec` scope's allocations can be safely released.
    /// Identical analysis to `let` — letrec's mutual recursion and two-phase
    /// initialization don't change the escape conditions.
    fn can_scope_allocate_letrec(&mut self, bindings: &[(Binding, Hir)], body: &Hir) -> bool {
        self.can_scope_allocate_let(bindings, body)
    }

    /// Determine if a `block` scope's allocations can be safely released.
    ///
    /// Blocks don't introduce bindings but bracket a scope of allocations.
    /// Conditions:
    /// 1. No expression in body can suspend
    /// 2. Body result is provably immediate
    /// 3. All break values targeting this block are safe immediates
    /// 4. No `set!` to non-local bindings (blocks have no own bindings)
    fn can_scope_allocate_block(&mut self, block_id: &BlockId, body: &[Hir]) -> bool {
        self.scope_stats.scopes_analyzed += 1;
        // Condition 1: no suspension
        if body.iter().any(|e| e.signal.may_suspend()) {
            self.scope_stats.rejected_suspends += 1;
            return false;
        }

        // Collect Define bindings from the block body. Although blocks
        // don't introduce let-style bindings, they can contain def/var
        // statements that create bindings whose values are heap-allocated
        // inside the scope. These must be tracked so result_is_safe
        // doesn't treat them as pre-scope outer bindings.
        let scope_bindings: Vec<(Binding, &Hir)> = body
            .iter()
            .filter_map(|e| match &e.kind {
                HirKind::Define { binding, value } => Some((*binding, value.as_ref())),
                _ => None,
            })
            .collect();

        // B2: result is immediate (empty body → nil → safe)
        if let Some(last) = body.last() {
            if !self.result_is_safe(last, &scope_bindings) {
                self.scope_stats.rejected_unsafe_result += 1;
                return false;
            }
        }

        // Condition 3: all break values targeting this block are safe immediates.
        if !self.all_break_values_safe(body, *block_id, &scope_bindings) {
            self.scope_stats.rejected_break += 1;
            return false;
        }

        // Condition 4: no dangerous outward mutation
        if body
            .iter()
            .any(|e| self.body_contains_dangerous_outward_set(e, &scope_bindings))
        {
            self.scope_stats.rejected_outward_set += 1;
            return false;
        }

        self.scope_stats.scopes_qualified += 1;
        true
    }

    /// Determine if a non-tail call's temporaries can be freed after the
    /// call returns via `RegionEnter`/`RegionExit` around the call.
    ///
    /// Safe when ALL conditions hold:
    /// 1. Callee is a known function that returns an immediate
    /// 2. Callee is rotation-safe (doesn't escape heap values)
    /// 3. Call doesn't suspend (conservative: no yielding anywhere)
    /// 4. At least one argument may heap-allocate (otherwise no benefit)
    /// 5. No spliced arguments (splice path builds an array; more complex)
    fn can_scope_allocate_call(
        &self,
        func: &Hir,
        args: &[CallArg],
        _call_signals: SignalBits,
    ) -> bool {
        // Must be a variable reference to a known function
        let HirKind::Var(binding) = &func.kind else {
            return false;
        };

        // Condition 1: no spliced args
        if args.iter().any(|a| a.spliced) {
            return false;
        }

        // Condition 2: callee returns an immediate.
        // Intrinsics and immediate primitives are already handled by
        // try_lower_intrinsic / emit inline — no call instruction to wrap.
        // We need the callee to be a user function in the precomputed map.
        if !self
            .callee_result_immediate
            .get(binding)
            .copied()
            .unwrap_or(false)
        {
            return false;
        }

        // Condition 3: callee doesn't escape heap values.
        // This subsumes the suspension check: a rotation-safe function
        // never yields/stores a non-immediate to external structures,
        // so values allocated in the caller's region cannot escape even
        // if the callee suspends.
        if !self
            .callee_rotation_safe
            .get(binding)
            .copied()
            .unwrap_or(false)
        {
            return false;
        }

        // Condition 4: argument evaluation must not suspend.
        // The callee's execution is covered by rotation-safety, but arg
        // expressions run in the caller's context before the call.
        if args.iter().any(|a| a.expr.signal.may_suspend()) {
            return false;
        }

        // Condition 5: at least one arg may heap-allocate
        // (if all args are immediates, the region has nothing to reclaim)
        args.iter().any(|a| !self.result_is_safe(&a.expr, &[]))
    }

    /// Precompute `callee_result_immediate` for all function definitions.
    ///
    /// Fixpoint iteration: seed all functions as "returns immediate",
    /// then iterate `result_is_safe(body, &[])` until stable. A function
    /// whose body calls another function that returns a non-immediate
    /// will converge to non-immediate.
    fn precompute_result_immediate(&mut self, hir: &Hir) {
        let mut defs: Vec<(Binding, &Hir)> = Vec::new();
        Self::collect_lambda_defs(hir, &mut defs);
        if defs.is_empty() {
            return;
        }

        // Seed: all functions optimistically return immediates.
        for &(binding, _) in &defs {
            self.callee_result_immediate.insert(binding, true);
        }

        // Iterate until stable.
        loop {
            let mut changed = false;
            for &(binding, body) in &defs {
                let is_imm = self.body_result_is_immediate(body);
                let was_imm = self.callee_result_immediate[&binding];
                if was_imm && !is_imm {
                    self.callee_result_immediate.insert(binding, false);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Check if a function body always returns an immediate value.
    ///
    /// Unlike `result_is_safe` (which checks if a value is safe to
    /// return from a scope-allocated let), this checks the actual
    /// return type. For tail calls, it checks whether the CALLEE
    /// returns an immediate (via `call_result_is_safe`), not just
    /// whether the args avoid scope bindings.
    fn body_result_is_immediate(&self, hir: &Hir) -> bool {
        match &hir.kind {
            // Literals: all immediates
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList => true,

            // Var: a parameter or captured variable — could be anything.
            // We cannot know the caller's argument types, so conservatively
            // return false. Only constant-like values (literals) are safe.
            HirKind::Var(_) => false,

            // Control flow: recurse into all result positions
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.body_result_is_immediate(then_branch)
                    && self.body_result_is_immediate(else_branch)
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses
                    .iter()
                    .all(|(_, body)| self.body_result_is_immediate(body))
                    && else_branch
                        .as_ref()
                        .is_none_or(|b| self.body_result_is_immediate(b))
            }
            HirKind::Begin(exprs) => exprs
                .last()
                .is_some_and(|e| self.body_result_is_immediate(e)),
            HirKind::And(exprs) | HirKind::Or(exprs) => {
                exprs.iter().all(|e| self.body_result_is_immediate(e))
            }
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                self.body_result_is_immediate(body)
            }
            HirKind::Block { body, .. } => body
                .last()
                .is_some_and(|e| self.body_result_is_immediate(e)),
            HirKind::Match { arms, .. } => arms
                .iter()
                .all(|(_, _, body)| self.body_result_is_immediate(body)),
            HirKind::While { .. } => true, // returns nil

            // ALL calls (tail or not): check if callee returns immediate
            HirKind::Call { func, args, .. } => self.call_result_is_safe(func, args),

            // Heap-allocating: Lambda, String, Quote, etc.
            _ => false,
        }
    }

    /// Precompute `callee_rotation_safe` for all function definitions.
    ///
    /// Walks the HIR to find all `Define` bindings with lambda values,
    /// then iterates `body_escapes_heap_values` until the map stabilizes.
    /// This handles mutual recursion: initially all functions are assumed
    /// safe, and each pass may flip some to unsafe. Converges because
    /// the only transition is safe→unsafe (monotone).
    fn precompute_rotation_safety(&mut self, hir: &Hir) {
        // Collect all (binding, lambda_body) pairs from the HIR.
        let mut defs: Vec<(Binding, &Hir)> = Vec::new();
        Self::collect_lambda_defs(hir, &mut defs);
        if defs.is_empty() {
            return;
        }

        // Seed: all functions optimistically safe.
        for &(binding, _) in &defs {
            self.callee_rotation_safe.insert(binding, true);
        }

        // Iterate until stable.
        loop {
            let mut changed = false;
            for &(binding, body) in &defs {
                let escapes = self.body_escapes_heap_values(body);
                let was_safe = self.callee_rotation_safe[&binding];
                if was_safe && escapes {
                    self.callee_rotation_safe.insert(binding, false);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Collect all `(binding, lambda_body)` pairs from Define nodes.
    fn collect_lambda_defs<'b>(hir: &'b Hir, out: &mut Vec<(Binding, &'b Hir)>) {
        match &hir.kind {
            HirKind::Define { binding, value } => {
                if let HirKind::Lambda { body, .. } = &value.kind {
                    out.push((*binding, body));
                }
                Self::collect_lambda_defs(value, out);
            }
            HirKind::Begin(exprs) => {
                for e in exprs {
                    Self::collect_lambda_defs(e, out);
                }
            }
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (binding, init) in bindings {
                    if let HirKind::Lambda { body: lbody, .. } = &init.kind {
                        out.push((*binding, lbody));
                    }
                    Self::collect_lambda_defs(init, out);
                }
                Self::collect_lambda_defs(body, out);
            }
            // Don't recurse into nested lambdas — they have their own
            // lowering context and precompute call.
            HirKind::Lambda { .. } => {}
            _ => {}
        }
    }

    /// Check if a HIR body is a tail call (or control flow where all result
    /// positions are tail calls). Used to relax the suspension check: a
    /// tail call replaces the frame, so its signal doesn't affect the
    /// enclosing scope's lifetime.
    fn body_is_tail_call(hir: &Hir) -> bool {
        match &hir.kind {
            HirKind::Call { is_tail: true, .. } => true,
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => Self::body_is_tail_call(then_branch) && Self::body_is_tail_call(else_branch),
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses
                    .iter()
                    .all(|(_, body)| Self::body_is_tail_call(body))
                    && else_branch
                        .as_ref()
                        .is_some_and(|b| Self::body_is_tail_call(b))
            }
            HirKind::Begin(exprs) => exprs.last().is_some_and(Self::body_is_tail_call),
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                Self::body_is_tail_call(body)
            }
            HirKind::Match { arms, .. } => arms
                .iter()
                .all(|(_, _, body)| Self::body_is_tail_call(body)),
            _ => false,
        }
    }

    /// Check if non-tail sub-expressions within a tail-call body may suspend.
    ///
    /// When `body_is_tail_call` returns true, the tail call's own signal
    /// is irrelevant (RegionExit fires before it). But preceding expressions
    /// in the body (e.g. side effects before the tail call in a `begin`)
    /// still execute within the scope and their suspension is dangerous.
    ///
    /// Returns true if any non-tail sub-expression may suspend.
    fn non_tail_subexprs_may_suspend(hir: &Hir) -> bool {
        match &hir.kind {
            // A bare tail call has no preceding expressions.
            HirKind::Call { is_tail: true, .. } => false,
            // If/Cond: the condition runs before branches.
            HirKind::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                cond.signal.may_suspend()
                    || Self::non_tail_subexprs_may_suspend(then_branch)
                    || Self::non_tail_subexprs_may_suspend(else_branch)
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses
                    .iter()
                    .any(|(c, b)| c.signal.may_suspend() || Self::non_tail_subexprs_may_suspend(b))
                    || else_branch
                        .as_ref()
                        .is_some_and(|b| Self::non_tail_subexprs_may_suspend(b))
            }
            // Begin: all expressions except the last are non-tail.
            HirKind::Begin(exprs) => {
                let non_tail = &exprs[..exprs.len().saturating_sub(1)];
                non_tail.iter().any(|e| e.signal.may_suspend())
                    || exprs
                        .last()
                        .is_some_and(Self::non_tail_subexprs_may_suspend)
            }
            // Let/Letrec: init expressions are non-tail; recurse into body.
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                bindings.iter().any(|(_, init)| init.signal.may_suspend())
                    || Self::non_tail_subexprs_may_suspend(body)
            }
            // Match: the scrutinee is non-tail; recurse into arm bodies.
            HirKind::Match { value, arms } => {
                value.signal.may_suspend()
                    || arms
                        .iter()
                        .any(|(_, _, body)| Self::non_tail_subexprs_may_suspend(body))
            }
            // Anything else that body_is_tail_call returned true for:
            // conservatively say it may suspend.
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::Span;

    fn make_span() -> Span {
        Span::new(0, 0, 1, 1)
    }

    #[test]
    fn test_lower_int() {
        let arena = crate::hir::BindingArena::new();
        let mut lowerer = Lowerer::new(&arena);
        let hir = Hir::silent(HirKind::Int(42), make_span());
        let func = lowerer.lower(&hir).unwrap();
        assert!(!func.blocks.is_empty());
    }

    #[test]
    fn test_lower_if() {
        let arena = crate::hir::BindingArena::new();
        let mut lowerer = Lowerer::new(&arena);
        let hir = Hir::silent(
            HirKind::If {
                cond: Box::new(Hir::silent(HirKind::Bool(true), make_span())),
                then_branch: Box::new(Hir::silent(HirKind::Int(1), make_span())),
                else_branch: Box::new(Hir::silent(HirKind::Int(2), make_span())),
            },
            make_span(),
        );
        let func = lowerer.lower(&hir).unwrap();
        // If now creates multiple blocks: entry, then, else, merge
        assert_eq!(func.blocks.len(), 4);
        // Entry block should have a Branch terminator
        assert!(matches!(
            func.blocks[0].terminator.terminator,
            Terminator::Branch { .. }
        ));
    }

    #[test]
    fn test_lower_begin() {
        let arena = crate::hir::BindingArena::new();
        let mut lowerer = Lowerer::new(&arena);
        let hir = Hir::silent(
            HirKind::Begin(vec![
                Hir::silent(HirKind::Int(1), make_span()),
                Hir::silent(HirKind::Int(2), make_span()),
            ]),
            make_span(),
        );
        let func = lowerer.lower(&hir).unwrap();
        assert!(!func.blocks.is_empty());
    }
}
