// audited: 2026-11-27
//! Where `elle_jit_yield` parks a local a nested closure captures: its cell in the env, nil on the stack.
//!
//! docs/impl/jit-closure.md
use super::*;

/// A local a nested closure captures parks in the env slot `populate_env`
/// gives it, `captures + local`, and its stack slot holds nil, so the
/// interpreter that resumes the frame reads the cell where its `LoadUpvalue`
/// looks (docs/impl/jit-closure.md). The compiled prologue holds the cell in
/// the local's variable, so the spill carries it among the locals.
///
/// The counter-factual is the frame built as captures then params: the env
/// ends at the params, the cell sits on the stack, and the resumed body's read
/// of the local reads past the env.
#[test]
fn a_celled_local_parks_in_its_env_slot() {
    crate::value::arena::with_test_region(|| {
        // 1 param, which owns local slot 0; locals 1 and 2 past it, and the
        // mask sets bit 1, local 2. One operand.
        let yield_meta =
            YieldPointMeta::new(7, SpillLayout::default().params(1).locals(3).operands(1));
        let (mut vm, closure_val) = setup_yield_test_with_lbox(
            vec![0xCC; 10],
            vec![],
            vec![Value::int(777)],
            vec![yield_meta],
            1,
            0,
            0b10,
        );

        // Spilled: [param0, local0 (the param's copy), local1, local2 (the
        // cell), op0]. An integer stands in for the cell: the layout is the
        // claim.
        let spilled: Vec<Value> = vec![
            Value::int(10),
            Value::int(11),
            Value::int(12),
            Value::int(13),
            Value::int(40),
        ];

        elle_jit_yield(
            Value::int(0).tag,
            Value::int(0).payload,
            spilled.as_ptr(),
            0,
            &mut vm as *mut crate::vm::VM as *mut () as u64,
            closure_val.tag,
            closure_val.payload,
            crate::value::fiber::SIG_YIELD.raw(),
        );

        let frame = as_bytecode_frame(&vm.fiber.suspended.as_ref().unwrap()[0]);
        // env = captures [777] + params [10] + locals past the params [nil, cell]
        let env: Vec<Option<i64>> = frame.env.iter().map(|v| v.as_int()).collect();
        assert_eq!(env, vec![Some(777), Some(10), None, Some(13)]);
        assert!(frame.env[2].is_nil(), "an uncelled local's env slot is nil");

        // stack = locals [11, 12, nil] + operands [40]
        let stack: Vec<Option<i64>> = frame.stack.iter().map(|v| v.as_int()).collect();
        assert_eq!(stack, vec![Some(11), Some(12), None, Some(40)]);
        assert!(
            frame.stack[2].is_nil(),
            "a celled local's stack slot is nil"
        );
    });
}
