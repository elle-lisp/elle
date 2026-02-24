//! HIR to LIR lowering

mod binding;
mod control;
mod expr;
mod lambda;
mod pattern;

use super::intrinsics::IntrinsicOp;
use super::types::*;
use crate::hir::{Binding, Hir, HirKind, HirPattern};
use crate::syntax::Span;
use crate::value::{Arity, SymbolId, Value};
use rustc_hash::FxHashMap;
use std::collections::HashMap;

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
    /// Compile-time constant values for immutable bindings (for LoadConst optimization)
    immutable_values: HashMap<Binding, Value>,
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
            immutable_values: HashMap::new(),
        }
    }

    /// Set intrinsic operations for operator specialization
    pub fn with_intrinsics(mut self, intrinsics: FxHashMap<SymbolId, IntrinsicOp>) -> Self {
        self.intrinsics = intrinsics;
        self
    }

    /// Lower a HIR expression to LIR
    pub fn lower(&mut self, hir: &Hir) -> Result<LirFunction, String> {
        self.current_func = LirFunction::new(Arity::Exact(0));
        self.current_block = BasicBlock::new(Label(0));
        self.next_reg = 0;
        self.next_label = 1;
        self.binding_to_slot.clear();

        let result_reg = self.lower_expr(hir)?;
        self.terminate(Terminator::Return(result_reg));
        self.finish_block();

        self.current_func.entry = Label(0);
        self.current_func.num_regs = self.next_reg;
        // Propagate effect from HIR to top-level LIR function
        self.current_func.effect = hir.effect;

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
        let hir = Hir::pure(HirKind::Int(42), make_span());
        let func = lowerer.lower(&hir).unwrap();
        assert!(!func.blocks.is_empty());
    }

    #[test]
    fn test_lower_if() {
        let mut lowerer = Lowerer::new();
        let hir = Hir::pure(
            HirKind::If {
                cond: Box::new(Hir::pure(HirKind::Bool(true), make_span())),
                then_branch: Box::new(Hir::pure(HirKind::Int(1), make_span())),
                else_branch: Box::new(Hir::pure(HirKind::Int(2), make_span())),
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
        let hir = Hir::pure(
            HirKind::Begin(vec![
                Hir::pure(HirKind::Int(1), make_span()),
                Hir::pure(HirKind::Int(2), make_span()),
            ]),
            make_span(),
        );
        let func = lowerer.lower(&hir).unwrap();
        assert!(!func.blocks.is_empty());
    }
}
