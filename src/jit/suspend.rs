//! Yield side-exit helpers for JIT-compiled code

use super::dispatch::YIELD_SENTINEL;
use crate::jit::value::JitValue;
use crate::value::{BytecodeFrame, SuspendedFrame, Value};

// =============================================================================
// Yield Side-Exit Helpers
// =============================================================================

/// JIT yield side-exit: build a SuspendedFrame and set fiber.signal.
///
/// Called from JIT code when a Yield terminator is reached.
///
/// Parameters:
///   yielded_tag/yielded_payload: the value being yielded
///   spilled_values: *const Value (16 bytes each), or null if nothing to spill
///   yield_index: index into JitCode.yield_points
///   vm: *mut () (raw VM pointer)
///   closure_tag/closure_payload: the closure being executed (for self-tail-call detection)
///
/// Returns YIELD_SENTINEL.
///
/// # Safety
/// `spilled_values` must point to `num_spilled` contiguous `Value`s
/// (or be null when num_spilled is 0).
#[no_mangle]
pub extern "C" fn elle_jit_yield(
    yielded_tag: u64,
    yielded_payload: u64,
    spilled_values: *const Value,
    yield_index: u64,
    vm: u64, // *mut () as u64
    closure_tag: u64,
    closure_payload: u64,
) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let yielded = Value {
        tag: yielded_tag,
        payload: yielded_payload,
    };
    let closure_val = Value {
        tag: closure_tag,
        payload: closure_payload,
    };

    let closure = closure_val
        .as_closure()
        .expect("VM bug: elle_jit_yield called with non-closure self");

    // Look up yield point metadata from JitCode
    let bytecode_ptr = closure.template.bytecode.as_ptr();
    let jit_code = vm
        .jit_cache
        .get(&bytecode_ptr)
        .expect("VM bug: elle_jit_yield called but no JitCode in cache");
    let yield_meta = &jit_code.yield_points[yield_index as usize];
    let num_locals = yield_meta.num_locals as usize;
    let num_operands = yield_meta.num_spilled as usize;
    let total_spilled = num_locals + num_operands;

    // Build the stack from spilled values.
    // The JIT spills in interpreter layout: [locals..., operands...].
    let mut stack = Vec::with_capacity(total_spilled);
    for i in 0..total_spilled {
        let v = unsafe { *spilled_values.add(i) };
        stack.push(v);
    }

    let frame = SuspendedFrame::Bytecode(BytecodeFrame {
        bytecode: closure.template.bytecode.clone(),
        constants: closure.template.constants.clone(),
        env: closure.env.clone(),
        ip: yield_meta.resume_ip,
        stack,
        location_map: closure.template.location_map.clone(),
        // JIT yield: on resume, the resume argument becomes the result of
        // the (yield ...) expression — push it onto the restored stack.
        push_resume_value: true,
    });

    vm.fiber.signal = Some((crate::value::fiber::SIG_YIELD, yielded));
    vm.fiber.suspended = Some(vec![frame]);

    YIELD_SENTINEL
}

/// JIT yield-through-call: append a caller frame to fiber.suspended.
///
/// Called from JIT code when a callee yields (detected by post-call
/// signal check). Builds a caller SuspendedFrame and appends it to
/// the existing suspended frame chain.
///
/// Parameters:
///   spilled_values: *const Value (16 bytes each)
///   call_site_index: index into JitCode.call_sites
///   vm: *mut () as u64
///   closure_tag/closure_payload: the closure being executed
///
/// Returns YIELD_SENTINEL.
///
/// # Safety
/// `spilled_values` must point to `num_spilled` contiguous `Value`s
/// (or be null when num_spilled is 0).
#[no_mangle]
pub extern "C" fn elle_jit_yield_through_call(
    spilled_values: *const Value,
    call_site_index: u64,
    vm: u64, // *mut () as u64
    closure_tag: u64,
    closure_payload: u64,
) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let closure_val = Value {
        tag: closure_tag,
        payload: closure_payload,
    };

    let closure = closure_val
        .as_closure()
        .expect("VM bug: elle_jit_yield_through_call called with non-closure");

    // Look up call site metadata from JitCode
    let bytecode_ptr = closure.template.bytecode.as_ptr();
    let jit_code = vm
        .jit_cache
        .get(&bytecode_ptr)
        .expect("VM bug: elle_jit_yield_through_call called but no JitCode in cache");
    let call_meta = &jit_code.call_sites[call_site_index as usize];

    let n = call_meta.num_spilled as usize;
    let mut stack = Vec::with_capacity(n);
    for i in 0..n {
        let v = unsafe { *spilled_values.add(i) };
        stack.push(v);
    }

    let caller_frame = SuspendedFrame::Bytecode(BytecodeFrame {
        bytecode: closure.template.bytecode.clone(),
        constants: closure.template.constants.clone(),
        env: closure.env.clone(),
        ip: call_meta.resume_ip,
        stack,
        location_map: closure.template.location_map.clone(),
        // JIT caller frame: on resume, the callee's return value flows as
        // current_value and must be pushed as the Call instruction's result.
        push_resume_value: true,
    });

    // Append caller frame to the existing suspended chain.
    let mut frames = vm.fiber.suspended.take().unwrap_or_default();
    frames.push(caller_frame);
    vm.fiber.suspended = Some(frames);

    YIELD_SENTINEL
}

/// Check if any actionable signal is pending on the VM.
/// Returns TRUE if set, FALSE otherwise.
///
/// This extends `elle_jit_has_exception` to also detect yield signals.
/// Used after Call instructions in yielding functions.
///
/// Uses bitwise containment (`contains`) rather than exact equality,
/// because I/O primitives return compound signals like `SIG_YIELD | SIG_IO`.
/// Exact-match would miss these, causing yield sentinels to leak into
/// registers as values.
#[no_mangle]
pub extern "C" fn elle_jit_has_signal(vm: u64) -> JitValue {
    let vm = unsafe { &*(vm as *const crate::vm::VM) };
    JitValue::bool_val(vm.fiber.signal.as_ref().is_some_and(|(b, _)| !b.is_ok()))
}

#[cfg(test)]
mod tests {
    use super::super::dispatch::YieldPointMeta;
    use super::*;
    use crate::value::fiber::SIG_YIELD;

    // =========================================================================
    // JIT yield: SuspendedFrame layout invariant
    // =========================================================================

    /// Set up a VM + Closure + JitCode for yield tests.
    /// Returns (vm, closure_value) with the JitCode already in jit_cache.
    fn setup_yield_test(
        bytecode: Vec<u8>,
        constants: Vec<Value>,
        env: Vec<Value>,
        yield_points: Vec<YieldPointMeta>,
    ) -> (crate::vm::VM, Value) {
        use crate::signals::Signal;
        use crate::value::types::Arity;
        use crate::value::ClosureTemplate;
        use std::collections::HashMap;
        use std::rc::Rc;

        let bytecode = Rc::new(bytecode);
        let constants = Rc::new(constants);
        let env = Rc::new(env);

        let template = Rc::new(ClosureTemplate {
            bytecode: bytecode.clone(),
            arity: Arity::Exact(0),
            num_locals: 0,
            num_captures: 0,
            num_params: 0,
            constants,
            signal: Signal::yields(),
            lbox_params_mask: 0,
            lbox_locals_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(crate::error::LocationMap::new()),
            jit_code: None,
            lir_function: None,
            doc: None,
            syntax: None,
            vararg_kind: crate::hir::VarargKind::List,
            name: None,
        });

        let closure = crate::value::Closure {
            template: template.clone(),
            env,
            squelch_mask: 0,
        };

        let bytecode_ptr = template.bytecode.as_ptr();
        let closure_val = Value::closure(closure);

        let jit_code = Rc::new(crate::jit::JitCode::test_with_yield_points(yield_points));

        let mut vm = crate::vm::VM::new();
        vm.jit_cache.insert(bytecode_ptr, jit_code);

        (vm, closure_val)
    }

    /// Extract the BytecodeFrame from a SuspendedFrame::Bytecode variant.
    fn as_bytecode_frame(frame: &SuspendedFrame) -> &BytecodeFrame {
        match frame {
            SuspendedFrame::Bytecode(f) => f,
            _ => panic!("expected SuspendedFrame::Bytecode"),
        }
    }

    #[test]
    fn test_jit_yield_builds_correct_suspended_frame() {
        // 2 params, 1 local, 3 operands
        let yield_meta = YieldPointMeta {
            resume_ip: 42,
            num_spilled: 3, // operand count
            num_locals: 3,  // params + locally-defined = 2 + 1
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
        );

        assert_eq!(result, YIELD_SENTINEL);

        let (sig, val) = vm.fiber.signal.unwrap();
        assert_eq!(sig, SIG_YIELD);
        assert_eq!(val.as_int(), Some(100));

        let frames = vm.fiber.suspended.as_ref().unwrap();
        assert_eq!(frames.len(), 1);
        let frame = as_bytecode_frame(&frames[0]);

        assert_eq!(frame.ip, 42);
        assert_eq!(&*frame.bytecode, &bytecode);
        assert_eq!(&*frame.constants, &constants);
        assert_eq!(&*frame.env, &env);

        assert_eq!(frame.stack.len(), 6);
        assert_eq!(frame.stack[0].as_int(), Some(10));
        assert_eq!(frame.stack[1].as_int(), Some(20));
        assert_eq!(frame.stack[2].as_int(), Some(30));
        assert_eq!(frame.stack[3].as_int(), Some(40));
        assert_eq!(frame.stack[4].as_int(), Some(50));
        assert_eq!(frame.stack[5].as_int(), Some(60));
    }

    #[test]
    fn test_jit_yield_zero_locals_zero_operands() {
        let yield_meta = YieldPointMeta {
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
        );

        assert_eq!(result, YIELD_SENTINEL);

        let frames = vm.fiber.suspended.as_ref().unwrap();
        let frame = as_bytecode_frame(&frames[0]);
        assert_eq!(frame.stack.len(), 0);
        assert_eq!(frame.ip, 0);
    }

    #[test]
    fn test_jit_yield_only_operands_no_locals() {
        let yield_meta = YieldPointMeta {
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
        );

        let frame = as_bytecode_frame(&vm.fiber.suspended.as_ref().unwrap()[0]);
        assert_eq!(frame.stack.len(), 2);
        assert_eq!(frame.stack[0].as_int(), Some(1));
        assert_eq!(frame.stack[1].as_int(), Some(2));
    }

    #[test]
    fn test_jit_yield_only_locals_no_operands() {
        let yield_meta = YieldPointMeta {
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
        );

        let frame = as_bytecode_frame(&vm.fiber.suspended.as_ref().unwrap()[0]);
        assert_eq!(frame.stack.len(), 3);
        assert_eq!(frame.stack[0].as_int(), Some(100));
        assert_eq!(frame.stack[1].as_int(), Some(200));
        assert_eq!(frame.stack[2].as_int(), Some(300));
    }

    #[test]
    fn test_jit_yield_large_spill() {
        let yield_meta = YieldPointMeta {
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
        );

        let frame = as_bytecode_frame(&vm.fiber.suspended.as_ref().unwrap()[0]);
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
    }

    #[test]
    fn test_jit_yield_multiple_yield_points() {
        let yield_points = vec![
            YieldPointMeta {
                resume_ip: 10,
                num_spilled: 1,
                num_locals: 2,
            },
            YieldPointMeta {
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
        );

        let frame = as_bytecode_frame(&vm.fiber.suspended.as_ref().unwrap()[0]);
        assert_eq!(frame.ip, 20);
        assert_eq!(frame.stack.len(), 4);
    }

    #[test]
    fn test_jit_yield_preserves_value_types() {
        let yield_meta = YieldPointMeta {
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
        );

        let frame = as_bytecode_frame(&vm.fiber.suspended.as_ref().unwrap()[0]);
        assert_eq!(frame.stack.len(), 4);
        assert!(frame.stack[0].is_nil());
        assert_eq!(frame.stack[1].as_bool(), Some(true));
        assert_eq!(frame.stack[2].as_float(), Some(1.5));
        assert!(frame.stack[3].is_empty_list());
    }
}
