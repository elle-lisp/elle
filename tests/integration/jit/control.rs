// audited: 2026-10-06
// docs/impl/jit.md
// Which values a compiled `Branch` reads as true, and what the compile gate
// accepts beside them.

use super::*;

// =============================================================================
// Control Flow Tests
// =============================================================================

/// fn(x) -> if x then 1 else 0
fn one_if_truthy() -> LirOwned {
    function(
        Arity::Exact(1),
        2,
        Signal::silent(),
        &[
            // Entry block: load arg, branch on x
            (
                0,
                &[load_arg(Reg(0), 0)],
                Terminator::Branch {
                    cond: Reg(0),
                    then_label: Label(1),
                    else_label: Label(2),
                },
            ),
            // Then block: return 1
            (
                1,
                &[InstrRef::Const {
                    dst: Reg(1),
                    value: ConstRef::Int(1),
                }],
                Terminator::Return(Reg(1)),
            ),
            // Else block: return 0
            (
                2,
                &[InstrRef::Const {
                    dst: Reg(1),
                    value: ConstRef::Int(0),
                }],
                Terminator::Return(Reg(1)),
            ),
        ],
    )
}

#[test]
fn test_jit_branch_true() {
    let result = compile_and_call(&one_if_truthy(), &[Value::TRUE]).unwrap();
    assert_eq!(result.as_int(), Some(1));
}

#[test]
fn test_jit_branch_false() {
    let result = compile_and_call(&one_if_truthy(), &[Value::FALSE]).unwrap();
    assert_eq!(result.as_int(), Some(0));
}

#[test]
fn test_jit_branch_nil() {
    // nil is falsy
    let result = compile_and_call(&one_if_truthy(), &[Value::NIL]).unwrap();
    assert_eq!(result.as_int(), Some(0));
}

#[test]
fn test_jit_branch_integer_truthy() {
    // Non-zero integers are truthy
    let result = compile_and_call(&one_if_truthy(), &[Value::int(42)]).unwrap();
    assert_eq!(result.as_int(), Some(1));
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[test]
fn test_jit_accepts_yielding() {
    let func = function(
        Arity::Exact(0),
        1,
        Signal::yields(),
        &[(
            0,
            &[InstrRef::Const {
                dst: Reg(0),
                value: ConstRef::Int(42),
            }],
            Terminator::Return(Reg(0)),
        )],
    );

    let compiler = JitCompiler::new().unwrap();
    let result = compiler.compile(&func.view());
    assert!(
        result.is_ok(),
        "JIT should accept yielding functions via side-exit: {:?}",
        result
    );
}

#[test]
fn test_jit_call_compiles() {
    // Test that Call instruction compiles
    let func = function(
        Arity::Exact(1),
        2,
        Signal::silent(),
        &[(
            0,
            &[
                load_arg(Reg(0), 0),
                InstrRef::Call {
                    dst: Reg(1),
                    func: Reg(0),
                    args: &[],
                    arity_checked: false,
                    region: elle::hir::region::StaticRegion::new(2).unwrap(),
                },
            ],
            Terminator::Return(Reg(1)),
        )],
    );

    let compiler = JitCompiler::new().unwrap();
    let result = compiler.compile(&func.view());
    // Call should now compile successfully
    assert!(result.is_ok(), "Call should compile: {:?}", result);
}

#[test]
fn test_jit_rejects_make_closure() {
    // MakeClosure is rejected at the gate: the JIT has no translation for it,
    // so a function holding one runs on the interpreter.
    let func = function(
        Arity::Exact(0),
        1,
        Signal::silent(),
        &[(
            0,
            &[InstrRef::MakeClosure {
                dst: Reg(0),
                closure_id: elle::lir::ClosureId(0),
                captures: &[],
                // A real per-execution slot (>= 2). The lowerer assigns real slots
                // to allocating instructions.
                region: elle::hir::region::StaticRegion::new(2).unwrap(),
            }],
            Terminator::Return(Reg(0)),
        )],
    );

    let compiler = JitCompiler::new().unwrap();
    let result = compiler.compile(&func.view());
    assert!(
        matches!(result, Err(elle::jit::JitError::UnsupportedInstruction(_))),
        "MakeClosure should be rejected: {:?}",
        result,
    );
}
