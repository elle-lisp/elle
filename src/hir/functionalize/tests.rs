//! Unit tests (`super` is the parent impl module).

use crate::pipeline::eval_all;
use crate::primitives::register_primitives;
use crate::symbol::SymbolTable;
use crate::value::Value;
use crate::vm::VM;

fn eval_bare(source: &str) -> Result<Value, String> {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    register_primitives(&mut vm, &mut symbols);
    // Thread a fresh per-call compile context.
    let mut cctx = crate::pipeline::CompileCtx::new();
    eval_all(source, &mut symbols, &mut vm, &mut cctx, "<test>")
}

#[test]
fn if_phi_merge_with_continuation_still_works() {
    // When the if is NOT the last expression, the phi-lets should
    // still correctly merge the assigned value for downstream use.
    let result = eval_bare(
        r#"(do
                    (var x 0)
                    (if true
                        (assign x 42)
                        (assign x 99))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(42));
}
