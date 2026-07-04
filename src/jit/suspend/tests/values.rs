use super::*;

#[test]
fn test_jit_yield_multiple_yield_points() {
    crate::value::arena::with_test_region(|| {
        let yield_points = vec![
            YieldPointMeta {
                num_params: 0,
                resume_ip: 10,
                num_spilled: 1,
                num_locals: 2,
            },
            YieldPointMeta {
                num_params: 0,
                resume_ip: 20,
                num_spilled: 3,
                num_locals: 1,
            },
        ];

        let (mut vm, closure_val) =
            setup_yield_test(vec![0x01, 0x02], vec![], vec![], yield_points);

        let spilled: Vec<Value> = vec![
            Value::int(10),
            Value::int(20),
            Value::int(30),
            Value::int(40),
        ];

        elle_jit_yield(
            Value::int(0).tag,
            Value::int(0).payload,
            spilled.as_ptr(),
            1,
            &mut vm as *mut crate::vm::VM as *mut () as u64,
            closure_val.tag,
            closure_val.payload,
            crate::value::fiber::SIG_YIELD.raw(),
        );

        let frame = as_bytecode_frame(&vm.fiber.suspended.as_ref().unwrap()[0]);
        assert_eq!(frame.ip, 20);
        // yield point 1: num_params=0, num_locals=1, num_spilled=3
        // env = captures(0) + params(0) = empty; stack = locals(1) + operands(3)
        assert_eq!(frame.env.len(), 0);
        assert_eq!(frame.stack.len(), 4);
    });
}

#[test]
fn test_jit_yield_preserves_value_types() {
    crate::value::arena::with_test_region(|| {
        let yield_meta = YieldPointMeta {
            num_params: 0,
            resume_ip: 0,
            num_spilled: 2,
            num_locals: 2,
        };

        let (mut vm, closure_val) = setup_yield_test(vec![0x01], vec![], vec![], vec![yield_meta]);

        let spilled: Vec<Value> = vec![
            Value::NIL,
            Value::bool(true),
            Value::float(1.5),
            Value::EMPTY_LIST,
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
        // env = captures(0) + params(0) = empty; stack = locals(2) + operands(2)
        assert_eq!(frame.env.len(), 0);
        assert_eq!(frame.stack.len(), 4);
        assert!(frame.stack[0].is_nil());
        assert_eq!(frame.stack[1].as_bool(), Some(true));
        assert_eq!(frame.stack[2].as_float(), Some(1.5));
        assert!(frame.stack[3].is_empty_list());
    });
}

#[test]
fn test_jit_yield_rewraps_lbox_locals() {
    crate::value::arena::with_test_region(|| {
        // 2 params (param 1 is lbox-wrapped), 2 locally-defined (local 0 is lbox-wrapped)
        // capture_params_mask = 0b10 (param index 1)
        // capture_locals_mask = 0b01 (local index 0)
        let yield_meta = YieldPointMeta {
            num_params: 2,
            resume_ip: 50,
            num_spilled: 1, // 1 operand
            num_locals: 2,  // 2 locally-defined
        };

        let (mut vm, closure_val) = setup_yield_test_with_lbox(
            vec![0xBB; 10],
            vec![],
            vec![], // no captures
            vec![yield_meta],
            2,    // num_params
            0b10, // capture_params_mask: param 1 is mutable-captured
            0b01, // capture_locals_mask: local 0 is mutable-captured
        );

        // Spilled: [param0=10, param1=20, local0=30, local1=40, op0=50]
        // JIT spills raw (unwrapped) values for all slots.
        let spilled: Vec<Value> = vec![
            Value::int(10),
            Value::int(20),
            Value::int(30),
            Value::int(40),
            Value::int(50),
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
        // env = captures(0) + params(2)
        assert_eq!(frame.env.len(), 2);
        assert_eq!(frame.env[0].as_int(), Some(10)); // param 0
        assert_eq!(frame.env[1].as_int(), Some(20)); // param 1

        // stack = locals(2) + operands(1)
        assert_eq!(frame.stack.len(), 3);
        assert_eq!(frame.stack[0].as_int(), Some(30)); // local 0
        assert_eq!(frame.stack[1].as_int(), Some(40)); // local 1
        assert_eq!(frame.stack[2].as_int(), Some(50)); // operand 0
    });
}
