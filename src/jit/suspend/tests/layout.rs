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
        assert_eq!(&*frame.code.bytecode, &bytecode);
        assert_eq!(&*frame.code.constants, &constants);
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
        // env = captures(0) + params(0) = empty
        assert_eq!(frame.env.len(), 0);
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
        // env = captures(0) + params(0) = empty
        assert_eq!(frame.env.len(), 0);
        // stack = locals(10) + operands(20)
        assert_eq!(frame.stack.len(), 30);
        for i in 0..30 {
            assert_eq!(
                frame.stack[i].as_int(),
                Some(i as i64),
                "stack[{}] mismatch",
                i
            );
        }
        assert_eq!(frame.ip, 99);
    });
}
