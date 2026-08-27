//! Assembling a `LirFunction` by hand, for the unit tests of the backends that
//! consume it.
//!
//! Every consumer of LIR — the bytecode emitter, the JIT, the WASM backend, the
//! MLIR and SPIR-V tiers, the cross-thread send path — tests against a function
//! written out instruction by instruction, because the shape under test is
//! usually one the front end cannot be coaxed into producing on demand. The
//! assembly is the same every time: name the function, open a block, push
//! spanned instructions, close it with a terminator.
//!
//! [`LirFixture`] is that assembly, once. It mirrors [`crate::hir::testkit`],
//! which does the same job for the front-end passes.
//!
//! The register count is inferred rather than declared: `num_regs` is one past
//! the highest register id the blocks mention, so it cannot drift away from the
//! instructions the way a hand-written constant does. A test that wants a count
//! the instructions do not justify says so with [`LirFixture::num_regs`].

use crate::lir::{
    for_each_def, for_each_terminator_use, for_each_use, BasicBlock, CallSiteInfo, ClosureId,
    Label, LirFunction, LirInstr, Reg, SpannedInstr, SpannedTerminator, Terminator, YieldPointInfo,
};
use crate::signals::Signal;
use crate::syntax::Span;
use crate::value::Arity;

/// Builds a [`LirFunction`].
///
/// See `src/lir/AGENTS.md` § "Building LIR in tests" for the rules; the pins
/// for each of them are at the bottom of this file.
pub(crate) struct LirFixture {
    func: LirFunction,
    /// The count [`LirFixture::num_regs`] asked for, if it was called. `None`
    /// leaves the count to `build`'s inference.
    declared_regs: Option<u32>,
}

impl LirFixture {
    /// A function of `arity` with no blocks and every `LirFunction::new`
    /// default: no name, silent signal, no captures, no locals.
    pub(crate) fn new(arity: Arity) -> Self {
        LirFixture {
            func: LirFunction::new(arity),
            declared_regs: None,
        }
    }

    pub(crate) fn name(mut self, name: &str) -> Self {
        self.func.name = Some(name.to_string());
        self
    }

    pub(crate) fn signal(mut self, signal: Signal) -> Self {
        self.func.signal = signal;
        self
    }

    pub(crate) fn num_captures(mut self, num_captures: u16) -> Self {
        self.func.num_captures = num_captures;
        self
    }

    pub(crate) fn num_locals(mut self, num_locals: u16) -> Self {
        self.func.num_locals = num_locals;
        self
    }

    pub(crate) fn num_params(mut self, num_params: usize) -> Self {
        self.func.num_params = num_params;
        self
    }

    pub(crate) fn closure_id(mut self, closure_id: ClosureId) -> Self {
        self.func.closure_id = Some(closure_id);
        self
    }

    pub(crate) fn yield_points(mut self, yield_points: Vec<YieldPointInfo>) -> Self {
        self.func.yield_points = yield_points;
        self
    }

    /// The per-call-site resume metadata a suspending function's backends index
    /// by call-site number. A `Call` inside a `may_suspend` function needs one
    /// entry per site, or the JIT's yield check fails translation.
    pub(crate) fn call_sites(mut self, call_sites: Vec<CallSiteInfo>) -> Self {
        self.func.call_sites = call_sites;
        self
    }

    /// Fix the register count instead of inferring it, for a test whose subject
    /// is the count itself.
    pub(crate) fn num_regs(mut self, num_regs: u32) -> Self {
        self.declared_regs = Some(num_regs);
        self
    }

    /// Append a block: `instrs` in order, then `terminator`. Every span is
    /// synthetic. The first block appended is the function's entry.
    pub(crate) fn block(
        mut self,
        label: u32,
        instrs: Vec<LirInstr>,
        terminator: Terminator,
    ) -> Self {
        let mut block = BasicBlock::new(Label(label));
        block.instructions = instrs
            .into_iter()
            .map(|instr| SpannedInstr::new(instr, Span::synthetic()))
            .collect();
        block.terminator = SpannedTerminator::new(terminator, Span::synthetic());
        if self.func.blocks.is_empty() {
            self.func.entry = block.label;
        }
        self.func.blocks.push(block);
        self
    }

    pub(crate) fn build(self) -> LirFunction {
        let mut func = self.func;
        func.num_regs = self.declared_regs.unwrap_or_else(|| registers_used(&func));
        func
    }
}

/// One past the highest register id `func`'s blocks mention — as a def, as a
/// use, or as a terminator's operand. Zero for a function that names none.
///
/// Uses count, not just defs: a test builds the shape it means to test, and a
/// register read but never written (a parameter the backend supplies, say)
/// still has to fit inside the count every backend indexes registers against.
fn registers_used(func: &LirFunction) -> u32 {
    let mut highest: Option<u32> = None;
    let mut note = |reg: Reg| highest = Some(highest.map_or(reg.0, |h: u32| h.max(reg.0)));
    for block in &func.blocks {
        for si in &block.instructions {
            for_each_def(&si.instr, &mut note);
            for_each_use(&si.instr, &mut note);
            // A `TailCall`'s result register is not among its defs: the WASM
            // backend, whose allocator the walkers serve, never materializes it.
            // The JIT does — it binds `dst` to the result of a normally-completing
            // native callee — so the count must still leave room for it.
            if let LirInstr::TailCall { dst, .. } = &si.instr {
                note(*dst);
            }
        }
        for_each_terminator_use(&block.terminator.terminator, &mut note);
    }
    highest.map_or(0, |h| h + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lir::{BinOp, LirConst};

    #[test]
    fn blocks_land_in_call_order_and_the_first_one_is_the_entry() {
        // The labels are neither sequential nor zero-based, so an entry read off
        // the first block cannot be confused with `LirFunction::new`'s default.
        let func = LirFixture::new(Arity::Exact(0))
            .block(5, vec![], Terminator::Jump(Label(7)))
            .block(7, vec![], Terminator::Unreachable)
            .build();
        assert_eq!(
            func.blocks.iter().map(|b| b.label).collect::<Vec<_>>(),
            vec![Label(5), Label(7)],
        );
        assert_eq!(
            func.entry,
            Label(5),
            "the first block appended is the entry"
        );
    }

    #[test]
    fn the_register_count_is_one_past_the_highest_register() {
        let func = LirFixture::new(Arity::Exact(0))
            .block(
                0,
                vec![
                    LirInstr::Const {
                        dst: Reg(0),
                        value: LirConst::Int(1),
                    },
                    LirInstr::Const {
                        dst: Reg(3),
                        value: LirConst::Int(2),
                    },
                    LirInstr::BinOp {
                        dst: Reg(1),
                        op: BinOp::Add,
                        lhs: Reg(0),
                        rhs: Reg(3),
                    },
                ],
                Terminator::Return(Reg(1)),
            )
            .build();
        assert_eq!(
            func.num_regs, 4,
            "Reg(3) is the highest register the block names",
        );
    }

    #[test]
    fn the_register_count_covers_a_register_only_a_terminator_names() {
        // A branch condition and a returned value are registers like any other:
        // a count read from the instructions alone would leave them outside it.
        let func = LirFixture::new(Arity::Exact(0))
            .block(
                0,
                vec![],
                Terminator::Branch {
                    cond: Reg(4),
                    then_label: Label(1),
                    else_label: Label(1),
                },
            )
            .block(1, vec![], Terminator::Return(Reg(2)))
            .build();
        assert_eq!(func.num_regs, 5, "the branch condition is Reg(4)");
    }

    #[test]
    fn the_register_count_covers_a_tail_calls_result_register() {
        // The JIT binds a `TailCall`'s `dst` when the callee turns out to be a
        // normally-completing native, and it indexes its argument variables from
        // `num_regs` — so a count that stopped at the operands would place an
        // argument on top of the result register.
        let func = LirFixture::new(Arity::Exact(1))
            .block(
                0,
                vec![LirInstr::TailCall {
                    dst: Reg(2),
                    func: Reg(0),
                    args: vec![Reg(1)],
                    arity_checked: false,
                    region: crate::hir::region::StaticRegion::new(2).unwrap(),
                    defer_callee_release: false,
                    deferred_release_slot: None,
                    borrowed_arg_slots: Vec::new(),
                }],
                Terminator::Unreachable,
            )
            .build();
        assert_eq!(func.num_regs, 3, "the result register is Reg(2)");
    }

    #[test]
    fn a_blockless_function_names_no_registers() {
        let func = LirFixture::new(Arity::Exact(1)).build();
        assert_eq!(func.num_regs, 0);
        assert!(func.blocks.is_empty());
    }

    #[test]
    fn a_declared_register_count_overrides_the_inference() {
        // The override exists for the tests whose subject is the count itself,
        // so it must survive a block whose instructions imply a different one.
        let func = LirFixture::new(Arity::Exact(0))
            .num_regs(9)
            .block(
                0,
                vec![LirInstr::Const {
                    dst: Reg(0),
                    value: LirConst::Int(1),
                }],
                Terminator::Return(Reg(0)),
            )
            .build();
        assert_eq!(
            func.num_regs, 9,
            "the declared count wins over the inferred 1",
        );
    }

    #[test]
    fn instructions_and_terminators_carry_synthetic_spans() {
        let func = LirFixture::new(Arity::Exact(0))
            .block(
                0,
                vec![LirInstr::Const {
                    dst: Reg(0),
                    value: LirConst::Nil,
                }],
                Terminator::Return(Reg(0)),
            )
            .build();
        let block = &func.blocks[0];
        assert_eq!(block.instructions[0].span, Span::synthetic());
        assert_eq!(block.terminator.span, Span::synthetic());
    }

    #[test]
    fn the_setters_write_their_fields() {
        let func = LirFixture::new(Arity::AtLeast(1))
            .name("f")
            .signal(Signal::yields())
            .num_captures(2)
            .num_locals(3)
            .num_params(4)
            .closure_id(ClosureId(5))
            .yield_points(vec![YieldPointInfo {
                resume_ip: 6,
                stack_regs: vec![],
                num_locals: 3,
            }])
            .build();
        assert_eq!(func.name.as_deref(), Some("f"));
        assert_eq!(func.signal, Signal::yields());
        assert_eq!(func.num_captures, 2);
        assert_eq!(func.num_locals, 3);
        assert_eq!(func.num_params, 4);
        assert_eq!(func.closure_id, Some(ClosureId(5)));
        assert_eq!(func.yield_points.len(), 1);
        assert_eq!(func.arity, Arity::AtLeast(1));
    }
}
