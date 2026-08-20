//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_reg_display() {
    assert_eq!(format!("{}", Reg(0)), "r0");
    assert_eq!(format!("{}", Reg(42)), "r42");
}

#[test]
fn test_label_display() {
    assert_eq!(format!("{}", Label(0)), "block0");
    assert_eq!(format!("{}", Label(5)), "block5");
}

#[test]
fn test_binop_display() {
    assert_eq!(format!("{}", BinOp::Add), "+");
    assert_eq!(format!("{}", BinOp::Shl), "<<");
}

#[test]
fn test_cmpop_display() {
    assert_eq!(format!("{}", CmpOp::Eq), "=");
    assert_eq!(format!("{}", CmpOp::Le), "≤");
}

#[test]
fn test_const_display() {
    assert_eq!(format!("{}", LirConst::Nil), "nil");
    assert_eq!(format!("{}", LirConst::Int(42)), "42");
    assert_eq!(format!("{}", LirConst::Keyword("lit".into())), ":lit");
    assert_eq!(format!("{}", LirConst::String("hello".into())), "\"hello\"");
}

#[test]
fn test_instr_const() {
    let instr = LirInstr::Const {
        dst: Reg(0),
        value: LirConst::Int(42),
    };
    assert_eq!(format!("{}", instr), "r0 ← 42");
}

#[test]
fn test_instr_binop() {
    let instr = LirInstr::BinOp {
        dst: Reg(2),
        op: BinOp::Add,
        lhs: Reg(0),
        rhs: Reg(1),
    };
    assert_eq!(format!("{}", instr), "r2 ← r0 + r1");
}

#[test]
fn test_instr_call() {
    let instr = LirInstr::Call {
        dst: Reg(5),
        func: Reg(3),
        args: vec![Reg(4)],
        arity_checked: false,
        region: crate::hir::region::StaticRegion::new(2).unwrap(),
    };
    assert_eq!(format!("{}", instr), "r5 ← r3(r4)");
}

#[test]
fn test_instr_call_multi_args() {
    let instr = LirInstr::Call {
        dst: Reg(5),
        func: Reg(3),
        args: vec![Reg(1), Reg(2)],
        arity_checked: false,
        region: crate::hir::region::StaticRegion::new(2).unwrap(),
    };
    assert_eq!(format!("{}", instr), "r5 ← r3(r1, r2)");
}

#[test]
fn test_instr_tailcall() {
    let instr = LirInstr::TailCall {
        dst: Reg(5),
        func: Reg(0),
        args: vec![Reg(1), Reg(2)],
        arity_checked: false,
        region: crate::hir::region::StaticRegion::new(2).unwrap(),
        defer_callee_release: false,
        deferred_release_slot: None,
        borrowed_arg_slots: Vec::new(),
    };
    assert_eq!(format!("{}", instr), "tailcall r0(r1, r2)");
}

#[test]
fn test_instr_compare() {
    let instr = LirInstr::Compare {
        dst: Reg(3),
        op: CmpOp::Lt,
        lhs: Reg(1),
        rhs: Reg(2),
    };
    assert_eq!(format!("{}", instr), "r3 ← r1 < r2");
}

#[test]
fn test_instr_type_check() {
    let instr = LirInstr::IsArray {
        dst: Reg(1),
        src: Reg(0),
    };
    assert_eq!(format!("{}", instr), "r1 ← tuple?(r0)");
}

#[test]
fn test_instr_destructuring() {
    assert_eq!(
        format!(
            "{}",
            LirInstr::ArrayMutRefDestructure {
                dst: Reg(2),
                src: Reg(0),
                index: 1
            }
        ),
        "r2 ← r0[1]!"
    );
    assert_eq!(
        format!(
            "{}",
            LirInstr::StructGetOrNil {
                dst: Reg(3),
                src: Reg(0),
                key: LirConst::Keyword("name".into())
            }
        ),
        "r3 ← r0.:name?"
    );
    assert_eq!(
        format!(
            "{}",
            LirInstr::StructGetDestructure {
                dst: Reg(3),
                src: Reg(0),
                key: LirConst::Keyword("name".into())
            }
        ),
        "r3 ← r0.:name!"
    );
}

#[test]
fn test_terminator_return() {
    assert_eq!(format!("{}", Terminator::Return(Reg(0))), "return r0");
}

#[test]
fn test_terminator_branch() {
    let term = Terminator::Branch {
        cond: Reg(2),
        then_label: Label(1),
        else_label: Label(3),
    };
    assert_eq!(format!("{}", term), "branch r2 → block1 / block3");
}

#[test]
fn test_terminator_emit() {
    let term = Terminator::Emit {
        signal: crate::value::fiber::SIG_YIELD,
        value: Reg(0),
        resume_label: Label(5),
    };
    assert_eq!(format!("{}", term), "emit 0x2 r0 → block5");
}

#[test]
fn test_terminator_kind() {
    assert_eq!(terminator_kind(&Terminator::Return(Reg(0))), "return");
    assert_eq!(terminator_kind(&Terminator::Jump(Label(0))), "jump");
    assert_eq!(
        terminator_kind(&Terminator::Branch {
            cond: Reg(0),
            then_label: Label(1),
            else_label: Label(2)
        }),
        "branch"
    );
}

#[test]
fn test_region_instructions() {
    assert_eq!(
        format!(
            "{}",
            LirInstr::DecrefRegion {
                region_id: crate::hir::region::StaticRegion::new(1).unwrap()
            }
        ),
        "decref-region 1"
    );
    assert_eq!(
        format!(
            "{}",
            LirInstr::IncrefRegion {
                region_id: crate::hir::region::StaticRegion::new(2).unwrap()
            }
        ),
        "incref-region 2"
    );
    assert_eq!(
        format!("{}", LirInstr::AdoptIntoActivation { child: Reg(3) }),
        "adopt-into-activation r3"
    );
}
