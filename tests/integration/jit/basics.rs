// audited: 2026-10-06
// docs/impl/jit.md
// What a compiled function returns when its body is an argument or one
// constant of each immediate kind.

use super::*;

/// A silent function whose one block runs `instrs` and returns `Reg(0)`.
fn returning_reg0(arity: usize, instrs: &[InstrRef<'_>]) -> LirOwned {
    function(
        Arity::Exact(arity),
        1,
        Signal::silent(),
        &[(0, instrs, Terminator::Return(Reg(0)))],
    )
}

/// A function of no arguments that returns `value`.
fn returning(value: ConstRef) -> LirOwned {
    returning_reg0(0, &[InstrRef::Const { dst: Reg(0), value }])
}

#[test]
fn test_jit_identity() {
    // fn(x) -> x
    let func = returning_reg0(1, &[load_arg(Reg(0), 0)]);
    let result = compile_and_call(&func, &[Value::int(42)]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_constant() {
    // fn() -> 42
    let result = compile_and_call(&returning(ConstRef::Int(42)), &[]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_nil() {
    // fn() -> nil
    let result = compile_and_call(&returning(ConstRef::Nil), &[]).unwrap();
    assert!(result.is_nil());
}

#[test]
fn test_jit_bool_true() {
    // fn() -> true
    let result = compile_and_call(&returning(ConstRef::Bool(true)), &[]).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_jit_bool_false() {
    // fn() -> false
    let result = compile_and_call(&returning(ConstRef::Bool(false)), &[]).unwrap();
    assert_eq!(result.as_bool(), Some(false));
}

#[test]
fn test_jit_empty_list() {
    // fn() -> ()
    let result = compile_and_call(&returning(ConstRef::EmptyList), &[]).unwrap();
    assert!(result.is_empty_list());
}
