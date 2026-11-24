// audited: 2026-11-23
//! Which calls to the running closure compile to a direct call, and where the
//! direct call's guards read the closure and the VM.
//!
//! docs/impl/jit-selfcall.md

use super::clif::{called_refs, func_refs};
use super::*;
use crate::hir::region::StaticRegion;
use crate::lir::{ConstRef, Offer, OfferBuf, Offers};

/// fn(n) -> self(args…). Reg(0) holds the parameter and Reg(1) the constant
/// 1. `arg` defines Reg(2), and Reg(3) holds the running closure.
fn self_call(arg: InstrRef<'_>, args: &[Reg], moves: Offers<'_>) -> LirOwned {
    LirFixture::new(Arity::Exact(1))
        .name("self-recursive")
        .signal(Signal::silent())
        .num_locals(1)
        .block(
            0,
            &[
                InstrRef::LoadCapture {
                    dst: Reg(0),
                    index: 0,
                },
                InstrRef::Const {
                    dst: Reg(1),
                    value: ConstRef::Int(1),
                },
                arg,
                InstrRef::LoadSelf { dst: Reg(3) },
                InstrRef::Call {
                    dst: Reg(4),
                    func: Reg(3),
                    args,
                    arity_checked: false,
                    region: StaticRegion::new(1).unwrap(),
                    moves,
                },
            ],
            Terminator::Return(Reg(4)),
        )
        .emitted_sites()
        .build()
}

/// `n - 1`, with the operands proven integers.
fn proven_arg() -> InstrRef<'static> {
    InstrRef::int_binop(Reg(2), BinOp::Sub, Reg(0), Reg(1))
}

/// `n - 1`, with nothing proven.
fn unproven_arg() -> InstrRef<'static> {
    InstrRef::binop(Reg(2), BinOp::Sub, Reg(0), Reg(1))
}

/// The `u0:N` id a rendered function names itself by, read off its signature
/// line (`function u0:3(i64, …) -> i64, i64 system_v {`).
fn own_func_id(clif: &[String]) -> u32 {
    clif.iter()
        .find_map(|line| {
            let rest = line.trim().strip_prefix("function ")?;
            let id = rest.split('(').next()?.trim().strip_prefix("u0:")?;
            id.parse::<u32>().ok()
        })
        .expect("a rendered function names itself")
}

/// Whether the rendered `lir` calls its own code, and whether it calls
/// `elle_jit_call`.
fn targets(lir: &LirOwned) -> (bool, bool) {
    let compiler = verified();
    let dispatch = compiler.helpers.call.as_u32();
    let clif = compiler
        .clif_text(&lir.view(), crate::jit::ChildTable::EMPTY)
        .expect("Failed to translate");
    let refs = func_refs(&clif);
    let called: Vec<u32> = called_refs(&clif)
        .iter()
        .filter_map(|name| refs.get(name).copied())
        .collect();
    (
        called.contains(&own_func_id(&clif)),
        called.contains(&dispatch),
    )
}

// Counter-factual: a translator that sends every call through the helper. It
// computes the same answers, and pays the helper's dispatch on every call of
// a self-recursion such as `fib`.
#[test]
fn a_self_call_with_an_int_proven_argument_calls_its_own_code() {
    let (direct, dispatch) = targets(&self_call(proven_arg(), &[Reg(2)], Offers::NONE));
    assert!(direct, "the call reaches the function's own code");
    assert!(
        dispatch,
        "a failed guard still sends the call through `elle_jit_call`"
    );
}

#[test]
fn a_self_call_with_a_constant_argument_calls_its_own_code() {
    let constant = InstrRef::Const {
        dst: Reg(2),
        value: ConstRef::Int(0),
    };
    let (direct, _) = targets(&self_call(constant, &[Reg(2)], Offers::NONE));
    assert!(direct, "a constant is an immediate, so the call is direct");
}

// Counter-factual: a translator that called its own code for every self-call.
// An unproven argument may be a heap value, whose owned parameter needs the
// reference `elle_jit_call` hands it, and the callee would release a reference
// it was never given.
#[test]
fn a_self_call_with_an_unproven_argument_calls_the_dispatch_helper() {
    let (direct, dispatch) = targets(&self_call(unproven_arg(), &[Reg(2)], Offers::NONE));
    assert!(!direct, "an unproven argument is not known to be immediate");
    assert!(dispatch, "the call goes through `elle_jit_call`");
}

// Counter-factual: a translator that read the arguments and not the offers.
// The caller's slot would wait for a word nobody writes.
#[test]
fn a_self_call_that_offers_its_argument_calls_the_dispatch_helper() {
    let mut moves = OfferBuf::default();
    moves.push(Offer::new(0, 0).unwrap());
    let (direct, dispatch) = targets(&self_call(proven_arg(), &[Reg(2)], moves.offers()));
    assert!(!direct, "an offer needs the hand-off word");
    assert!(dispatch, "the call goes through `elle_jit_call`");
}

// Counter-factual: a translator that took the arity on trust. The callee's
// prologue would read a second argument the caller never wrote, and the
// arity error `elle_jit_call` raises would never be raised.
#[test]
fn a_self_call_with_the_wrong_argument_count_calls_the_dispatch_helper() {
    let (direct, dispatch) = targets(&self_call(proven_arg(), &[Reg(2), Reg(1)], Offers::NONE));
    assert!(!direct, "two arguments to a function of one");
    assert!(dispatch, "the call goes through `elle_jit_call`");
}

/// A closure value whose squelch mask is `mask`, on `vm`'s heap.
fn closure_value(vm: &mut crate::vm::VM, mask: crate::value::SignalBits) -> crate::value::Value {
    let template =
        crate::value::CodeBuilder::new(Vec::new(), Arity::Exact(0), Vec::new()).build(vm.heap());
    let closure = crate::value::Closure::new(
        template,
        crate::value::region_slice::RegionSlice::empty(),
        mask,
    );
    let region = vm.heap().new_runtime_region();
    crate::value::build::closure(vm.heap(), closure, region)
}

// Counter-factual: `offset_of!(Closure, squelch_mask)` alone. It forgets the
// tag word the object starts with, and reads the eight bytes before the mask.
#[test]
fn a_closure_objects_squelch_mask_sits_at_the_offset_compiled_code_reads() {
    use crate::jit::translate::selfcall::SQUELCH_IN_CLOSURE_OBJECT;
    let mut vm = crate::vm::VM::new();
    let mask = crate::value::SIG_YIELD;
    let value = closure_value(&mut vm, mask);
    let base = value.payload as *const u8;
    // SAFETY: the payload addresses the live closure object, and the offset
    // names a word inside it.
    let read = unsafe { *(base.offset(SQUELCH_IN_CLOSURE_OBJECT as isize) as *const u64) };
    assert_eq!(
        read,
        mask.raw(),
        "the word at the offset is the closure's squelch mask"
    );
}

// Counter-factual: an offset into the `Fiber` alone, which forgets where the
// fiber sits inside the `VM`. The depth guard would compare two other words.
#[test]
fn the_call_depth_and_the_cap_sit_at_the_offsets_compiled_code_reads() {
    use crate::jit::translate::selfcall::{CALL_DEPTH_IN_VM, MAX_DEPTH_IN_VM};
    let mut vm = crate::vm::VM::new();
    vm.fiber.call_depth = 1234;
    vm.runtime_config.max_depth = 5678;
    let base = &vm as *const crate::vm::VM as *const u8;
    // SAFETY: both offsets lie inside the `VM`, and each names a `usize`.
    let (depth, cap) = unsafe {
        (
            *(base.offset(CALL_DEPTH_IN_VM as isize) as *const usize),
            *(base.offset(MAX_DEPTH_IN_VM as isize) as *const usize),
        )
    };
    assert_eq!(
        depth, 1234,
        "the word at the depth offset is the call depth"
    );
    assert_eq!(cap, 5678, "the word at the cap offset is the depth cap");
}
