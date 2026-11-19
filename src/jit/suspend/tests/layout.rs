// audited: 2026-11-18
//! The frame `elle_jit_yield` parks: the env holds captures, params and a slot
//! per local past them, and locals then operands fill its stack.
//!
//! docs/impl/jit.md
//! docs/impl/jit-closure.md
use super::*;

#[test]
fn test_jit_yield_builds_correct_suspended_frame() {
    crate::value::arena::with_test_region(|| {
        // 2 params, 1 local, 3 operands
        let yield_meta = YieldPointMeta {
            num_params: 2,
            resume_ip: 42,
            num_spilled: 3, // operand count
            num_locals: 1,  // 1 locally-defined
        };

        let bytecode = vec![0xAA; 10];
        let constants = vec![Value::int(999)];
        let env = vec![Value::int(777)];

        let (mut vm, closure_val) = setup_yield_test(
            bytecode.clone(),
            constants.clone(),
            env.clone(),
            vec![yield_meta],
        );

        // Spilled buffer: [param0, param1, local0, op0, op1, op2]
        let spilled: Vec<Value> = vec![
            Value::int(10),
            Value::int(20),
            Value::int(30),
            Value::int(40),
            Value::int(50),
            Value::int(60),
        ];

        let yielded = Value::int(100);

        let result = elle_jit_yield(
            yielded.tag,
            yielded.payload,
            spilled.as_ptr(),
            0,
            &mut vm as *mut crate::vm::VM as *mut () as u64,
            closure_val.tag,
            closure_val.payload,
            crate::value::fiber::SIG_YIELD.raw(),
        );

        assert_eq!(result, YIELD_SENTINEL);

        let (sig, val) = vm.fiber.signal.unwrap();
        assert_eq!(sig, SIG_YIELD);
        assert_eq!(val.as_int(), Some(100));

        let frames = vm.fiber.suspended.as_ref().unwrap();
        assert_eq!(frames.len(), 1);
        let frame = as_bytecode_frame(&frames[0]);

        assert_eq!(frame.ip, 42);
        assert_eq!(frame.code.bytecode(), &bytecode);
        assert_eq!(frame.code.constants(), &constants);
        // env = captures [777] + params [10, 20]
        assert_eq!(frame.env.len(), 3);
        assert_eq!(frame.env[0].as_int(), Some(777));
        assert_eq!(frame.env[1].as_int(), Some(10));
        assert_eq!(frame.env[2].as_int(), Some(20));

        // stack = locals [30] + operands [40, 50, 60]
        assert_eq!(frame.stack.len(), 4);
        assert_eq!(frame.stack[0].as_int(), Some(30));
        assert_eq!(frame.stack[1].as_int(), Some(40));
        assert_eq!(frame.stack[2].as_int(), Some(50));
        assert_eq!(frame.stack[3].as_int(), Some(60));
    });
}

#[test]
fn test_jit_yield_zero_locals_zero_operands() {
    crate::value::arena::with_test_region(|| {
        let yield_meta = YieldPointMeta {
            num_params: 0,
            resume_ip: 0,
            num_spilled: 0,
            num_locals: 0,
        };

        let (mut vm, closure_val) = setup_yield_test(vec![], vec![], vec![], vec![yield_meta]);

        let spilled: Vec<Value> = vec![];
        let yielded = Value::NIL;

        let result = elle_jit_yield(
            yielded.tag,
            yielded.payload,
            spilled.as_ptr(),
            0,
            &mut vm as *mut crate::vm::VM as *mut () as u64,
            closure_val.tag,
            closure_val.payload,
            crate::value::fiber::SIG_YIELD.raw(),
        );

        assert_eq!(result, YIELD_SENTINEL);

        let frames = vm.fiber.suspended.as_ref().unwrap();
        let frame = as_bytecode_frame(&frames[0]);
        assert_eq!(frame.stack.len(), 0);
        assert_eq!(frame.ip, 0);
    });
}

#[test]
fn test_jit_yield_only_operands_no_locals() {
    crate::value::arena::with_test_region(|| {
        let yield_meta = YieldPointMeta {
            num_params: 0,
            resume_ip: 10,
            num_spilled: 2,
            num_locals: 0,
        };

        let (mut vm, closure_val) = setup_yield_test(vec![0x01], vec![], vec![], vec![yield_meta]);

        let spilled: Vec<Value> = vec![Value::int(1), Value::int(2)];

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
        assert_eq!(frame.stack.len(), 2);
        assert_eq!(frame.stack[0].as_int(), Some(1));
        assert_eq!(frame.stack[1].as_int(), Some(2));
    });
}

#[test]
fn test_jit_yield_only_locals_no_operands() {
    crate::value::arena::with_test_region(|| {
        let yield_meta = YieldPointMeta {
            num_params: 0,
            resume_ip: 5,
            num_spilled: 0,
            num_locals: 3,
        };

        let (mut vm, closure_val) = setup_yield_test(vec![0x02], vec![], vec![], vec![yield_meta]);

        let spilled: Vec<Value> = vec![Value::int(100), Value::int(200), Value::int(300)];

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
        // env = captures(0) + params(0) + one slot per local past the params,
        // nil where no cell holds the local, as `populate_env` builds it
        assert_eq!(frame.env.len(), 3);
        assert!(frame.env.iter().all(|v| v.is_nil()));
        // stack = locals [100, 200, 300] + operands(0)
        assert_eq!(frame.stack.len(), 3);
        assert_eq!(frame.stack[0].as_int(), Some(100));
        assert_eq!(frame.stack[1].as_int(), Some(200));
        assert_eq!(frame.stack[2].as_int(), Some(300));
    });
}

#[test]
fn test_jit_yield_large_spill() {
    crate::value::arena::with_test_region(|| {
        let yield_meta = YieldPointMeta {
            num_params: 0,
            resume_ip: 99,
            num_spilled: 20,
            num_locals: 10,
        };

        let (mut vm, closure_val) = setup_yield_test(vec![0xFF], vec![], vec![], vec![yield_meta]);

        let spilled: Vec<Value> = (0..30).map(Value::int).collect();

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
        // env = captures(0) + params(0) + ten nil local slots
        assert_eq!(frame.env.len(), 10);
        assert!(frame.env.iter().all(|v| v.is_nil()));
        // stack = locals(10) + operands(20)
        assert_eq!(frame.stack.len(), 30);
        for (i, slot) in frame.stack.iter().enumerate() {
            assert_eq!(slot.as_int(), Some(i as i64), "stack[{}] mismatch", i);
        }
        assert_eq!(frame.ip, 99);
    });
}

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
        let yield_meta = YieldPointMeta {
            num_params: 1,
            resume_ip: 7,
            num_spilled: 1,
            num_locals: 3,
        };
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
