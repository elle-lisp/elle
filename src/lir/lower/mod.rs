//! HIR to LIR lowering

mod binding;
mod control;
pub(crate) mod decision;
mod escape;
mod expr;
mod lambda;
mod pattern;

use super::intrinsics::IntrinsicOp;
use super::types::*;
use crate::hir::{Binding, BlockId, Hir, HirKind, HirPattern};
use crate::syntax::Span;
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
pub struct Lowerer {
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
    /// Set of bindings that are upvalues (captures/parameters in lambda)
    /// These use LoadCapture/StoreCapture, not LoadLocal/StoreLocal
    upvalue_bindings: std::collections::HashSet<Binding>,
    /// Current span for emitted instructions
    current_span: Span,
    /// Intrinsic operations for operator specialization.
    /// Maps global SymbolId to specialized LIR instruction.
    intrinsics: FxHashMap<SymbolId, IntrinsicOp>,
    /// Primitives known to return NaN-boxed immediates.
    /// Used by escape analysis (`result_is_safe`) to accept calls to
    /// these primitives in scope-allocated let bodies.
    immediate_primitives: FxHashSet<SymbolId>,
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
}

impl Lowerer {
    pub fn new() -> Self {
        Lowerer {
            current_func: LirFunction::new(Arity::Exact(0)),
            current_block: BasicBlock::new(Label(0)),
            next_reg: 0,
            next_label: 1, // 0 is entry
            binding_to_slot: HashMap::new(),
            in_lambda: false,
            num_captures: 0,
            upvalue_bindings: std::collections::HashSet::new(),
            current_span: Span::synthetic(),
            intrinsics: FxHashMap::default(),
            immediate_primitives: FxHashSet::default(),
            immutable_values: HashMap::new(),
            block_lower_contexts: Vec::new(),
            region_depth: 0,
            scope_stats: ScopeStats::default(),
            discard_slot: None,
            symbol_names: HashMap::new(),
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
        // Inside a lambda, slots need to account for the captures offset
        // Environment layout: [captures..., params..., locally_defined...]
        // num_locals tracks params + locally_defined (NOT captures)
        // But binding_to_slot needs the actual index in the environment
        let slot = if self.in_lambda {
            // Track which locally-defined variables need cells.
            // Local index = num_locals - num_params (0-based within locally-defined vars).
            // Must use num_params (not arity.fixed_params()) because num_params includes
            // the rest parameter slot for variadic functions, matching the environment layout.
            let num_params = self.current_func.num_params as u16;
            let local_index = self.current_func.num_locals - num_params;
            if binding.needs_lbox() && local_index < 64 {
                self.current_func.lbox_locals_mask |= 1 << local_index;
            }
            self.num_captures + self.current_func.num_locals
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
    /// 3. Body result is provably a NaN-boxed immediate (not heap-allocated)
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
        if bindings.iter().any(|(b, _)| b.is_captured()) {
            self.scope_stats.rejected_captured += 1;
            return false;
        }

        // Condition 2: no suspension
        if body.signal.may_suspend() {
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
        // A break inside the let body emits compensating RegionExits that pop
        // the let's region mark. If the break value is heap-allocated inside
        // the scope, the RegionExit frees it → use-after-free. If the break
        // value is an immediate, the RegionExit doesn't affect it → safe.
        if !self.all_breaks_have_safe_values(body) {
            self.scope_stats.rejected_break += 1;
            return false;
        }

        // Condition 6: no escaping break. A break targeting a block outside
        // this let jumps past the let's RegionExit. While compensating exits
        // handle cleanup, the conservative approach avoids scope allocation
        // entirely when breaks escape. Breaks targeting blocks defined inside
        // the let body are safe — they stay within the scope's region.
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

        // B2: result is immediate (empty body → nil → safe)
        // Blocks have no bindings, so scope_bindings is empty — any Var
        // references something from outside and is safe to return.
        if let Some(last) = body.last() {
            if !self.result_is_safe(last, &[]) {
                self.scope_stats.rejected_unsafe_result += 1;
                return false;
            }
        }

        // Condition 3: all break values targeting this block are safe immediates.
        // Pass empty scope_bindings — blocks have no bindings of their own,
        // but `all_break_values_safe` extends scope_bindings as it recurses
        // into nested let/letrec nodes, so break values referencing inner
        // let bindings with heap inits are correctly rejected.
        if !self.all_break_values_safe(body, *block_id, &[]) {
            self.scope_stats.rejected_break += 1;
            return false;
        }

        // Condition 4: no dangerous outward mutation (blocks have no own bindings,
        // so any set is outward — but harmless if value is immediate)
        if body
            .iter()
            .any(|e| self.body_contains_dangerous_outward_set(e, &[]))
        {
            self.scope_stats.rejected_outward_set += 1;
            return false;
        }

        self.scope_stats.scopes_qualified += 1;
        true
    }
}

impl Default for Lowerer {
    fn default() -> Self {
        Self::new()
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
        let mut lowerer = Lowerer::new();
        let hir = Hir::inert(HirKind::Int(42), make_span());
        let func = lowerer.lower(&hir).unwrap();
        assert!(!func.blocks.is_empty());
    }

    #[test]
    fn test_lower_if() {
        let mut lowerer = Lowerer::new();
        let hir = Hir::inert(
            HirKind::If {
                cond: Box::new(Hir::inert(HirKind::Bool(true), make_span())),
                then_branch: Box::new(Hir::inert(HirKind::Int(1), make_span())),
                else_branch: Box::new(Hir::inert(HirKind::Int(2), make_span())),
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
        let mut lowerer = Lowerer::new();
        let hir = Hir::inert(
            HirKind::Begin(vec![
                Hir::inert(HirKind::Int(1), make_span()),
                Hir::inert(HirKind::Int(2), make_span()),
            ]),
            make_span(),
        );
        let func = lowerer.lower(&hir).unwrap();
        assert!(!func.blocks.is_empty());
    }
}
