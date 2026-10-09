// audited: 2026-10-06
// src/lir/AGENTS.md
//! Assembling a frozen function by hand, for the unit tests of the backends that
//! read it.
//!
//! Every reader of LIR tests against a function written out instruction by
//! instruction, because the shape under test is usually one the front end cannot
//! be coaxed into producing on demand. [`LirFixture`] is that assembly, once. It
//! mirrors [`crate::hir::testkit`], which does the same job for the front end.
//!
//! The register count is inferred rather than declared: `num_regs` is one past
//! the highest register id the blocks mention, so it cannot drift away from the
//! instructions the way a hand-written constant does. A test that wants a count
//! the instructions do not justify says so with [`LirFixture::num_regs`].

use crate::lir::code::{InstrRef, LirOwned, LirView};
use crate::lir::{
    for_each_terminator_use, CallSiteInfo, ClosureId, Label, LirBuilder, LirHead, Reg, Terminator,
    YieldPointInfo,
};
use crate::signals::Signal;
use crate::syntax::Span;
use crate::value::fiberheap::FiberHeap;
use crate::value::Arity;

/// Builds a frozen function through a [`LirBuilder`] over a heap of its own.
///
/// The rules are in `src/lir/AGENTS.md`; the test for each is at the bottom of
/// this file.
pub(crate) struct LirFixture {
    /// Declared before `heap`, so it drops first: it borrows `heap`.
    builder: LirBuilder<'static>,
    /// Boxed, so its address holds while the builder borrows it.
    #[allow(dead_code)]
    heap: Box<FiberHeap>,
    /// Whether a block has been appended, which decides the entry.
    entered: bool,
    /// The count [`LirFixture::num_regs`] asked for, if it was called. `None`
    /// leaves the count to `build`'s inference.
    declared_regs: Option<u32>,
    /// The sites emission would record, written into the frozen function.
    yield_points: Vec<YieldPointInfo>,
    call_sites: Vec<CallSiteInfo>,
}

impl LirFixture {
    /// A function of `arity` with no blocks and every default: no name, silent
    /// signal, no captures, no locals.
    pub(crate) fn new(arity: Arity) -> Self {
        let mut heap = Box::new(FiberHeap::new());
        // SAFETY: the heap is boxed, so its address is stable for the box's
        // life, and `builder` is declared first, so it drops before `heap`.
        let borrowed: &'static mut FiberHeap = unsafe { &mut *(heap.as_mut() as *mut FiberHeap) };
        let mut builder = LirBuilder::new(borrowed);
        builder.begin_function(arity);
        LirFixture {
            builder,
            heap,
            entered: false,
            declared_regs: None,
            yield_points: Vec::new(),
            call_sites: Vec::new(),
        }
    }

    /// Write any header field the setters below do not name.
    pub(crate) fn head(mut self, f: impl FnOnce(&mut LirHead)) -> Self {
        f(self.builder.head());
        self
    }

    pub(crate) fn name(self, name: &str) -> Self {
        self.head(|h| h.name = Some(name.to_string()))
    }

    pub(crate) fn signal(self, signal: Signal) -> Self {
        self.head(|h| h.signal = signal)
    }

    pub(crate) fn num_captures(self, num_captures: u16) -> Self {
        self.head(|h| h.num_captures = num_captures)
    }

    pub(crate) fn num_locals(self, num_locals: u16) -> Self {
        self.head(|h| h.num_locals = num_locals)
    }

    pub(crate) fn num_params(self, num_params: usize) -> Self {
        self.head(|h| h.num_params = num_params)
    }

    pub(crate) fn num_local_params(self, num_local_params: usize) -> Self {
        self.head(|h| h.num_local_params = num_local_params)
    }

    pub(crate) fn capture_params_mask(self, mask: u64) -> Self {
        self.head(|h| h.capture_params_mask = mask)
    }

    pub(crate) fn vararg_kind(self, kind: crate::hir::VarargKind) -> Self {
        self.head(|h| h.vararg_kind = kind)
    }

    pub(crate) fn closure_id(self, closure_id: ClosureId) -> Self {
        self.head(|h| h.closure_id = Some(closure_id))
    }

    pub(crate) fn yield_points(mut self, yield_points: Vec<YieldPointInfo>) -> Self {
        self.yield_points = yield_points;
        self
    }

    /// The per-call-site resume metadata a suspending function's backends index
    /// by call-site number. A `Call` inside a `may_suspend` function needs one
    /// entry per site, or the JIT's yield check fails translation.
    pub(crate) fn call_sites(mut self, call_sites: Vec<CallSiteInfo>) -> Self {
        self.call_sites = call_sites;
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
        instrs: &[InstrRef<'_>],
        terminator: Terminator,
    ) -> Self {
        if !self.entered {
            self.builder.head().entry = Label(label);
            self.entered = true;
        }
        self.builder.open_block(Label(label));
        for instr in instrs {
            self.builder.emit(*instr, Span::synthetic());
        }
        self.builder.terminate(terminator, Span::synthetic());
        self.builder.finish_block();
        self
    }

    /// The frozen function, with the sites the fixture was given.
    pub(crate) fn build(mut self) -> LirOwned {
        let mut owned = self.builder.finish_function().expect("a fixture freezes");
        owned.code.num_regs = self
            .declared_regs
            .unwrap_or_else(|| registers_used(&owned.view()));
        owned.set_sites(&self.yield_points, &self.call_sites);
        owned
    }
}

/// One past the highest register id `func`'s blocks mention — as a def, as a
/// use, or as a terminator's operand. Zero for a function that names none.
///
/// Uses count, not just defs: a test builds the shape it means to test, and a
/// register read but never written (a parameter the backend supplies, say)
/// still has to fit inside the count every backend indexes registers against.
fn registers_used(func: &LirView<'_>) -> u32 {
    let mut highest: Option<u32> = None;
    let mut note = |reg: Reg| highest = Some(highest.map_or(reg.0, |h: u32| h.max(reg.0)));
    for block in func.blocks() {
        for node in block.nodes() {
            node.def().into_iter().for_each(&mut note);
            node.uses().iter().copied().for_each(&mut note);
            // A `TailCall`'s result register is not among its defs: the WASM
            // backend, whose allocator the walkers serve, never materializes it.
            // The JIT does — it binds `dst` to the result of a normally-completing
            // native callee — so the count must still leave room for it.
            if let InstrRef::TailCall { dst, .. } = node.instr() {
                note(dst);
            }
        }
        for_each_terminator_use(&block.terminator(), &mut note);
    }
    highest.map_or(0, |h| h + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lir::{BinOp, ConstRef, Slots};

    #[test]
    fn blocks_land_in_call_order_and_the_first_one_is_the_entry() {
        // The labels are neither sequential nor zero-based, so an entry read off
        // the first block cannot be confused with the default.
        let func = LirFixture::new(Arity::Exact(0))
            .block(5, &[], Terminator::Jump(Label(7)))
            .block(7, &[], Terminator::Unreachable)
            .build();
        assert_eq!(
            func.view().blocks().map(|b| b.label()).collect::<Vec<_>>(),
            vec![Label(5), Label(7)],
        );
        assert_eq!(
            func.view().entry(),
            Label(5),
            "the first block appended is the entry"
        );
    }

    #[test]
    fn the_register_count_is_one_past_the_highest_register() {
        let func = LirFixture::new(Arity::Exact(0))
            .block(
                0,
                &[
                    InstrRef::Const {
                        dst: Reg(0),
                        value: ConstRef::Int(1),
                    },
                    InstrRef::Const {
                        dst: Reg(3),
                        value: ConstRef::Int(2),
                    },
                    InstrRef::binop(Reg(1), BinOp::Add, Reg(0), Reg(3)),
                ],
                Terminator::Return(Reg(1)),
            )
            .build();
        assert_eq!(
            func.view().num_regs(),
            4,
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
                &[],
                Terminator::Branch {
                    cond: Reg(4),
                    then_label: Label(1),
                    else_label: Label(1),
                },
            )
            .block(1, &[], Terminator::Return(Reg(2)))
            .build();
        assert_eq!(func.view().num_regs(), 5, "the branch condition is Reg(4)");
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
                &[InstrRef::TailCall {
                    dst: Reg(2),
                    func: Reg(0),
                    args: &[Reg(1)],
                    arity_checked: false,
                    region: crate::hir::region::StaticRegion::new(2).unwrap(),
                    defer_callee_release: false,
                    deferred_release_slot: None,
                    borrowed_arg_slots: Slots::new(&[]),
                }],
                Terminator::Unreachable,
            )
            .build();
        assert_eq!(func.view().num_regs(), 3, "the result register is Reg(2)");
    }

    #[test]
    fn a_blockless_function_names_no_registers() {
        let func = LirFixture::new(Arity::Exact(1)).build();
        assert_eq!(func.view().num_regs(), 0);
        assert_eq!(func.view().block_count(), 0);
    }

    #[test]
    fn a_declared_register_count_overrides_the_inference() {
        // The override exists for the tests whose subject is the count itself,
        // so it must survive a block whose instructions imply a different one.
        let func = LirFixture::new(Arity::Exact(0))
            .num_regs(9)
            .block(
                0,
                &[InstrRef::Const {
                    dst: Reg(0),
                    value: ConstRef::Int(1),
                }],
                Terminator::Return(Reg(0)),
            )
            .build();
        assert_eq!(
            func.view().num_regs(),
            9,
            "the declared count wins over the inferred 1",
        );
    }

    #[test]
    fn instructions_and_terminators_carry_synthetic_spans() {
        let func = LirFixture::new(Arity::Exact(0))
            .block(
                0,
                &[InstrRef::Const {
                    dst: Reg(0),
                    value: ConstRef::Nil,
                }],
                Terminator::Return(Reg(0)),
            )
            .build();
        let view = func.view();
        let block = view.block(0);
        assert_eq!(block.node(0).span(), Span::synthetic());
        assert_eq!(block.terminator_span(), Span::synthetic());
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
        let func = func.view();
        assert_eq!(func.name(), Some("f"));
        assert_eq!(func.signal(), Signal::yields());
        assert_eq!(func.num_captures(), 2);
        assert_eq!(func.num_locals(), 3);
        assert_eq!(func.num_params(), 4);
        assert_eq!(func.closure_id(), Some(ClosureId(5)));
        assert_eq!(func.yield_points().len(), 1);
        assert_eq!(func.arity(), Arity::AtLeast(1));
    }
}
