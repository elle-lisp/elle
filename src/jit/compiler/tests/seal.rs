// audited: 2026-10-20
// docs/impl/jit.md
//! When the translator seals a block, read off the block parameters a
//! translation builds, and a loop that still compiles to the right answer.

use super::*;
use crate::hir::region::StaticRegion;
use crate::lir::{CmpOp, ConstRef};
use cranelift_codegen::ir::{Function, UserFuncName, ValueDef};

/// fn() of `locals` locals: `blocks` blocks in a chain, each calling the
/// function itself. The function may yield, so each call parks on a signal,
/// and each park spills every local.
fn chain(blocks: u32, locals: u16) -> LirOwned {
    let mut fixture = LirFixture::new(Arity::Exact(0))
        .signal(Signal::yields())
        .num_locals(locals);
    for block in 0..blocks {
        let terminator = if block + 1 == blocks {
            Terminator::Return(Reg(1))
        } else {
            Terminator::Jump(Label(block + 1))
        };
        fixture = fixture.block(
            block,
            &[
                InstrRef::LoadSelf { dst: Reg(0) },
                InstrRef::Call {
                    dst: Reg(1),
                    func: Reg(0),
                    args: &[],
                    arity_checked: false,
                    region: StaticRegion::new(1).unwrap(),
                },
            ],
            terminator,
        );
    }
    fixture.emitted_sites().build()
}

/// How many block parameters translating `lir` builds. A parameter the seal
/// finds redundant becomes an alias, which keeps its entry, so the count
/// holds every parameter the translation built, not only the ones it kept.
fn parameters_built(lir: &LirOwned) -> usize {
    let mut compiler = JitCompiler::new().expect("Failed to create compiler");
    let mut func =
        Function::with_name_signature(UserFuncName::user(0, 0), compiler.make_jit_signature());
    compiler
        .translate_function(&lir.view(), &mut func)
        .expect("Failed to translate");
    let dfg = &func.dfg;
    dfg.values()
        .filter(|v| dfg.value_is_alias(*v) || matches!(dfg.value_def(*v), ValueDef::Param(..)))
        .count()
}

#[test]
fn a_local_costs_the_same_parameters_however_many_blocks_read_it() {
    // Trap: the IR a translation leaves is the same whenever the blocks are
    // sealed, because the seal removes every parameter its predecessors agree
    // on. Only the values the translation built show the parameters it made.
    // The count takes the parameters alone: each spill stores every local
    // through a `stack_addr`, so the spill code grows with blocks times locals.
    //
    // Counter-factual: a translator that seals every block at the end gives
    // each block a parameter for each local its spills read. That costs time
    // and memory in proportion to blocks times locals, and this count grows
    // with the chain.
    let locals = 32;
    let cost =
        |blocks| parameters_built(&chain(blocks, locals)) - parameters_built(&chain(blocks, 0));
    let (short, long) = (cost(4), cost(64));
    assert_eq!(
        long, short,
        "{locals} locals cost {short} parameters across 4 blocks and {long} across 64"
    );
}

/// fn(n): count a local from 0 up to `n` and return it. The test block is
/// the target of the loop's back edge.
fn make_counting_loop_lir() -> LirOwned {
    LirFixture::new(Arity::Exact(1))
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
                    value: ConstRef::Int(0),
                },
                InstrRef::StoreLocal {
                    slot: 0,
                    src: Reg(1),
                },
            ],
            Terminator::Jump(Label(1)),
        )
        .block(
            1,
            &[
                InstrRef::LoadLocal {
                    dst: Reg(2),
                    slot: 0,
                },
                InstrRef::compare(Reg(3), CmpOp::Lt, Reg(2), Reg(0)),
            ],
            Terminator::Branch {
                cond: Reg(3),
                then_label: Label(2),
                else_label: Label(3),
            },
        )
        .block(
            2,
            &[
                InstrRef::LoadLocal {
                    dst: Reg(4),
                    slot: 0,
                },
                InstrRef::Const {
                    dst: Reg(5),
                    value: ConstRef::Int(1),
                },
                InstrRef::binop(Reg(6), BinOp::Add, Reg(4), Reg(5)),
                InstrRef::StoreLocal {
                    slot: 0,
                    src: Reg(6),
                },
            ],
            Terminator::Jump(Label(1)),
        )
        .block(
            3,
            &[InstrRef::LoadLocal {
                dst: Reg(7),
                slot: 0,
            }],
            Terminator::Return(Reg(7)),
        )
        .build()
}

#[test]
fn a_loop_whose_header_waits_for_its_back_edge_counts_up() {
    // Counter-factual: a translator that sealed the loop's test block before
    // translating the block that jumps back to it would declare a predecessor
    // on a sealed block. The local's value from the back edge then never
    // reaches the test, and the loop never ends or returns its first value.
    let lir = make_counting_loop_lir();
    let code = JitCompiler::new()
        .expect("Failed to create compiler")
        .compile(&lir.view())
        .expect("Failed to compile");
    let mut vm = crate::vm::VM::new();
    let args = [crate::value::Value::int(10)];
    let value = unsafe {
        code.call(
            std::ptr::null(),
            args.as_ptr(),
            1,
            &mut vm as *mut crate::vm::VM as *mut (),
            0,
            0,
        )
    }
    .to_value();
    assert_eq!(value.as_int(), Some(10));
}
