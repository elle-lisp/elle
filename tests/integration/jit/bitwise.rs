// audited: 2026-10-06
// src/jit/AGENTS.md
// What JIT-compiled bitwise and logical operations return, and what an
// expression spanning several blocks returns.

use super::*;

// =============================================================================
// Complex Expression Tests
// =============================================================================

#[test]
fn test_jit_conditional_arithmetic() {
    // fn(x) -> if (x = 0) then 1 else (x * 2)
    let func = function(
        Arity::Exact(1),
        4,
        Signal::silent(),
        &[
            // Entry: load arg, compare x == 0
            (
                0,
                &[
                    load_arg(Reg(0), 0),
                    InstrRef::Const {
                        dst: Reg(1),
                        value: ConstRef::Int(0),
                    },
                    InstrRef::compare(Reg(2), CmpOp::Eq, Reg(0), Reg(1)),
                ],
                Terminator::Branch {
                    cond: Reg(2),
                    then_label: Label(1),
                    else_label: Label(2),
                },
            ),
            // Then: return 1
            (
                1,
                &[InstrRef::Const {
                    dst: Reg(3),
                    value: ConstRef::Int(1),
                }],
                Terminator::Return(Reg(3)),
            ),
            // Else: return x * 2
            (
                2,
                &[
                    InstrRef::Const {
                        dst: Reg(1),
                        value: ConstRef::Int(2),
                    },
                    InstrRef::binop(Reg(3), BinOp::Mul, Reg(0), Reg(1)),
                ],
                Terminator::Return(Reg(3)),
            ),
        ],
    );

    // Test x = 0 -> 1
    let result = compile_and_call(&func, &[Value::int(0)]).unwrap();
    assert_eq!(result.as_int(), Some(1));

    // Test x = 5 -> 10
    let result2 = compile_and_call(&func, &[Value::int(5)]).unwrap();
    assert_eq!(result2.as_int(), Some(10));
}

#[test]
fn test_jit_chained_arithmetic() {
    // fn(a, b, c) -> (a + b) * c
    let func = function(
        Arity::Exact(3),
        5,
        Signal::silent(),
        &[(
            0,
            &[
                load_arg(Reg(0), 0),
                load_arg(Reg(1), 1),
                load_arg(Reg(2), 2),
                InstrRef::binop(Reg(3), BinOp::Add, Reg(0), Reg(1)),
                InstrRef::binop(Reg(4), BinOp::Mul, Reg(3), Reg(2)),
            ],
            Terminator::Return(Reg(4)),
        )],
    );

    // (2 + 5) * 6 = 42
    let result = compile_and_call(&func, &[Value::int(2), Value::int(5), Value::int(6)]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

// =============================================================================
// Bitwise Operation Tests
// =============================================================================

#[test]
fn test_jit_bit_and() {
    let func = binary(InstrRef::binop(Reg(2), BinOp::BitAnd, Reg(0), Reg(1)));
    // 0b1111 & 0b1010 = 0b1010 = 10
    let result = compile_and_call(&func, &[Value::int(15), Value::int(10)]).unwrap();
    assert_eq!(result.as_int(), Some(10));
}

#[test]
fn test_jit_bit_or() {
    let func = binary(InstrRef::binop(Reg(2), BinOp::BitOr, Reg(0), Reg(1)));
    // 0b1100 | 0b0011 = 0b1111 = 15
    let result = compile_and_call(&func, &[Value::int(12), Value::int(3)]).unwrap();
    assert_eq!(result.as_int(), Some(15));
}

#[test]
fn test_jit_shl() {
    let func = binary(InstrRef::binop(Reg(2), BinOp::Shl, Reg(0), Reg(1)));
    // 1 << 4 = 16
    let result = compile_and_call(&func, &[Value::int(1), Value::int(4)]).unwrap();
    assert_eq!(result.as_int(), Some(16));
}

// =============================================================================
// Logical Operation Tests
// =============================================================================

#[test]
fn test_jit_not_true() {
    let func = unary(InstrRef::unary(Reg(1), UnaryOp::Not, Reg(0)));
    let result = compile_and_call(&func, &[Value::TRUE]).unwrap();
    assert_eq!(result.as_bool(), Some(false));
}

#[test]
fn test_jit_not_false() {
    let func = unary(InstrRef::unary(Reg(1), UnaryOp::Not, Reg(0)));
    let result = compile_and_call(&func, &[Value::FALSE]).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_jit_not_nil() {
    let func = unary(InstrRef::unary(Reg(1), UnaryOp::Not, Reg(0)));
    let result = compile_and_call(&func, &[Value::NIL]).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}
