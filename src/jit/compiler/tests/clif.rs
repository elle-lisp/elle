// audited: 2026-11-23
// docs/impl/jit.md
//! What only the rendered Cranelift IR settles: a load's flags, an operation's
//! tag test, and a call's target.

use super::*;

/// fn() -> capture 0. With `num_captures = 1`, `LoadCapture` index 0 reads
/// through the closure environment pointer rather than an argument variable.
fn make_capture_read_lir() -> LirOwned {
    LirFixture::new(Arity::Exact(0))
        .signal(Signal::silent())
        .num_captures(1)
        .block(
            0,
            &[InstrRef::LoadCapture {
                dst: Reg(0),
                index: 0,
            }],
            Terminator::Return(Reg(0)),
        )
        .build()
}

/// The `load` lines of a rendered Cranelift function, in emission order.
fn load_lines(clif: &[String]) -> Vec<&str> {
    clif.iter()
        .map(|line| line.trim())
        .filter(|line| line.contains("= load."))
        .collect()
}

/// fn(a, b) -> a `op` b, with the two arguments loaded from the argument array
/// and the operation built by `make_op`.
fn make_arith_lir(op: BinOp, make_op: fn(Reg, BinOp, Reg, Reg) -> InstrRef<'static>) -> LirOwned {
    LirFixture::new(Arity::Exact(2))
        .signal(Signal::silent())
        .block(
            0,
            &[
                InstrRef::LoadCapture {
                    dst: Reg(0),
                    index: 0,
                },
                InstrRef::LoadCapture {
                    dst: Reg(1),
                    index: 1,
                },
                make_op(Reg(2), op, Reg(0), Reg(1)),
            ],
            Terminator::Return(Reg(2)),
        )
        .build()
}

/// The `brif` lines of a rendered Cranelift function.
fn branch_lines(clif: &[String]) -> Vec<&str> {
    clif.iter()
        .map(|line| line.trim())
        .filter(|line| line.starts_with("brif "))
        .collect()
}

fn arith_clif(op: BinOp, make_op: fn(Reg, BinOp, Reg, Reg) -> InstrRef<'static>) -> Vec<String> {
    verified()
        .clif_text(
            &make_arith_lir(op, make_op).view(),
            crate::jit::ChildTable::EMPTY,
        )
        .expect("Failed to translate")
}

/// Without a proof the JIT cannot know the operands are integers, so it emits
/// the tag-check diamond: test both tags, then the inline integer instruction
/// or a call to the runtime helper (docs/impl/jit.md).
#[test]
fn an_unproven_arithmetic_op_compiles_to_a_tag_check_diamond() {
    // The counter-factual for the test below: a translator that never emitted
    // the diamond would pass "a proven op has no branch" trivially, while
    // computing garbage for every float operand that reaches an unproven site.
    for op in [BinOp::Add, BinOp::Sub, BinOp::Mul] {
        let clif = arith_clif(op, InstrRef::binop);
        assert!(
            !branch_lines(&clif).is_empty(),
            "{op:?}: an unproven op must test its operands' tags; got:\n{}",
            clif.join("\n")
        );
    }
}

/// A proven op skips the tag check: the operands are integers, so the branch
/// only ever takes one arm and the other arm is unreachable code
/// (docs/impl/lir.md).
#[test]
fn a_proven_arithmetic_op_compiles_without_a_tag_check() {
    for op in [BinOp::Add, BinOp::Sub, BinOp::Mul] {
        let clif = arith_clif(op, InstrRef::int_binop);
        let branches = branch_lines(&clif);
        assert!(
            branches.is_empty(),
            "{op:?}: a proven op must carry no type-check branch; got {branches:?} in:\n{}",
            clif.join("\n")
        );
    }
}

/// A proven comparison skips its tag check too — the same diamond, over the
/// comparison helpers (docs/impl/lir.md).
#[test]
fn a_proven_comparison_compiles_without_a_tag_check() {
    use crate::lir::CmpOp;

    for op in [CmpOp::Lt, CmpOp::Le, CmpOp::Eq] {
        let unproven = verified()
            .clif_text(
                &LirFixture::new(Arity::Exact(2))
                    .signal(Signal::silent())
                    .block(
                        0,
                        &[
                            InstrRef::LoadCapture {
                                dst: Reg(0),
                                index: 0,
                            },
                            InstrRef::LoadCapture {
                                dst: Reg(1),
                                index: 1,
                            },
                            InstrRef::compare(Reg(2), op, Reg(0), Reg(1)),
                        ],
                        Terminator::Return(Reg(2)),
                    )
                    .build()
                    .view(),
                crate::jit::ChildTable::EMPTY,
            )
            .expect("Failed to translate");
        assert!(
            !branch_lines(&unproven).is_empty(),
            "{op:?}: an unproven comparison must test its operands' tags"
        );

        let proven = verified()
            .clif_text(
                &LirFixture::new(Arity::Exact(2))
                    .signal(Signal::silent())
                    .block(
                        0,
                        &[
                            InstrRef::LoadCapture {
                                dst: Reg(0),
                                index: 0,
                            },
                            InstrRef::LoadCapture {
                                dst: Reg(1),
                                index: 1,
                            },
                            InstrRef::int_compare(Reg(2), op, Reg(0), Reg(1)),
                        ],
                        Terminator::Return(Reg(2)),
                    )
                    .build()
                    .view(),
                crate::jit::ChildTable::EMPTY,
            )
            .expect("Failed to translate");
        let branches = branch_lines(&proven);
        assert!(
            branches.is_empty(),
            "{op:?}: a proven comparison must carry no type-check branch; got {branches:?} in:\n{}",
            proven.join("\n")
        );
    }
}

/// A proven division keeps one branch — the zero test. Cranelift's `sdiv` traps
/// rather than returning a value, and the proof names the operands' type only
/// (docs/impl/jit.md).
#[test]
fn a_proven_division_keeps_its_zero_test() {
    let clif = arith_clif(BinOp::Div, InstrRef::int_binop);
    assert_eq!(
        branch_lines(&clif).len(),
        1,
        "a proven division keeps exactly the zero test; got:\n{}",
        clif.join("\n")
    );
}

#[test]
fn an_argument_load_carries_trusted_flags() {
    // Trap: memory flags change the access the backend emits, never the value
    // it computes, so nothing but the rendered CLIF shows which flags a load
    // actually got.
    //
    // Counter-factual: passing `MemFlagsData::new()` instead of `::trusted()`
    // compiles, and the compiled code returns the right answers, because the
    // argument array really is aligned and mapped. It costs a trapping,
    // unaligned-tolerant access on every parameter of every hot function.
    let compiler = verified();
    let clif = compiler
        .clif_text(&make_simple_lir().view(), crate::jit::ChildTable::EMPTY)
        .expect("Failed to translate");
    let loads = load_lines(&clif);
    assert!(
        !loads.is_empty(),
        "a one-parameter function loads its argument; got:\n{}",
        clif.join("\n")
    );
    for load in &loads {
        assert!(
            load.contains("notrap aligned"),
            "argument load without trusted flags: {load}"
        );
    }
}

#[test]
fn a_capture_load_carries_trusted_flags() {
    // The environment pointer is a second base pointer, reached from a
    // different translator path than the argument array.
    let compiler = verified();
    let clif = compiler
        .clif_text(
            &make_capture_read_lir().view(),
            crate::jit::ChildTable::EMPTY,
        )
        .expect("Failed to translate");
    let loads = load_lines(&clif);
    assert!(
        !loads.is_empty(),
        "reading capture 0 loads from the env pointer; got:\n{}",
        clif.join("\n")
    );
    for load in &loads {
        assert!(
            load.contains("notrap aligned"),
            "capture load without trusted flags: {load}"
        );
    }
}

/// `fnN` → the module function id it names, read off a rendered function's
/// preamble lines (`fn3 = u0:87 sig3`, optionally `colocated`).
pub(super) fn func_refs(clif: &[String]) -> HashMap<String, u32> {
    let mut refs = HashMap::new();
    for line in clif {
        let line = line.trim();
        let Some((name, rest)) = line.split_once(" = ") else {
            continue;
        };
        if !name.starts_with("fn") {
            continue;
        }
        let Some(id) = rest
            .split_whitespace()
            .find_map(|tok| tok.strip_prefix("u0:"))
        else {
            continue;
        };
        if let Ok(id) = id.parse::<u32>() {
            refs.insert(name.to_string(), id);
        }
    }
    refs
}

/// The `fnN` of the call instruction nearest above `at`, searching back only
/// within `at`'s own block.
pub(super) fn call_target_before(clif: &[String], at: usize) -> Option<String> {
    for line in clif[..at].iter().rev() {
        let line = line.trim();
        if line.starts_with("block") {
            return None;
        }
        let Some(pos) = line.find("call ") else {
            continue;
        };
        let rest = &line[pos + "call ".len()..];
        let name = rest.split('(').next()?.trim();
        return Some(name.to_string());
    }
    None
}

/// Every `fnN` a rendered function calls, in emission order.
pub(super) fn called_refs(clif: &[String]) -> Vec<String> {
    clif.iter()
        .filter_map(|line| {
            let pos = line.find("call ")?;
            let name = line[pos + "call ".len()..].split('(').next()?.trim();
            name.starts_with("fn").then(|| name.to_string())
        })
        .collect()
}
