// audited: 2026-10-12
//! The frame `elle_jit_fuel_pause` parks: at the charging instruction, with the
//! interpreter's layout, and with no resume value.
//!
//! docs/impl/jit.md

use super::*;
use crate::compiler::bytecode::Instruction;
use crate::jit::dispatch::ChargeSiteMeta;
use crate::value::fiber::SIG_FUEL;
use std::sync::Arc;

/// `setup_yield_test`'s VM and closure, with the cache entry re-keyed to a
/// JitCode carrying `charge_sites`.
fn setup_pause_test(
    bytecode: Vec<u8>,
    env: Vec<Value>,
    charge_sites: Vec<ChargeSiteMeta>,
) -> (crate::vm::VM, Value) {
    let (mut vm, closure_val) = setup_yield_test(bytecode, vec![], env, vec![]);
    let template = (*closure_val
        .as_closure()
        .expect("setup builds a closure")
        .template)
        .clone();
    vm.install_jit_code(
        template,
        Arc::new(crate::jit::JitCode::test_with_charge_sites(charge_sites)),
    );
    (vm, closure_val)
}

#[test]
fn a_fuel_pause_parks_the_frame_at_the_charging_instruction() {
    crate::value::arena::with_test_region(|| {
        // 1 param, 1 local, 2 operands: a call's argument and its callee.
        let (mut vm, closure_val) = setup_pause_test(
            vec![0xAA; 20],
            vec![Value::int(777)],
            vec![ChargeSiteMeta {
                ip: 17,
                num_spilled: 2,
                num_locals: 1,
                num_params: 1,
            }],
        );
        let spilled = [
            Value::int(10),
            Value::int(30),
            Value::int(40),
            Value::int(50),
        ];

        let result = elle_jit_fuel_pause(
            spilled.as_ptr(),
            0,
            &mut vm as *mut crate::vm::VM as *mut () as u64,
            closure_val.tag,
            closure_val.payload,
        );

        assert_eq!(result, YIELD_SENTINEL);
        let (sig, val) = vm.fiber.signal.expect("the pause sets the signal");
        assert_eq!(sig, SIG_FUEL, "a pause is the :fuel signal");
        assert!(val.is_nil(), "a pause carries a nil payload");

        let frames = vm
            .fiber
            .suspended
            .as_ref()
            .expect("the pause parks a frame");
        assert_eq!(frames.len(), 1);
        let frame = as_bytecode_frame(&frames[0]);
        assert_eq!(frame.ip, 17, "the frame parks at the charging instruction");
        assert!(
            !frame.push_resume_value,
            "the resume runs the instruction again, so it pushes no value",
        );
        // env = captures [777] + params [10]
        assert_eq!(
            frame.env.iter().map(|v| v.as_int()).collect::<Vec<_>>(),
            vec![Some(777), Some(10)],
        );
        // stack = locals [30] + operands [40, 50]
        assert_eq!(
            frame.stack.iter().map(|v| v.as_int()).collect::<Vec<_>>(),
            vec![Some(30), Some(40), Some(50)],
        );
    });
}

/// The parked instruction runs on resume against the stack the pause spilled,
/// and the value `fiber/resume` hands over goes nowhere.
#[test]
fn a_resumed_pause_runs_the_charging_instruction_on_the_spilled_stack() {
    // Counter-factual: a park that pushes the resume value, as a yield's does,
    // runs the instruction with that value on top. Here the instruction is a
    // `Return`, and the function returns 99, the resume value, instead of 42.
    crate::value::arena::with_test_region(|| {
        let (mut vm, closure_val) = setup_pause_test(
            vec![Instruction::Return as u8],
            vec![],
            vec![ChargeSiteMeta {
                ip: 0,
                num_spilled: 1,
                num_locals: 0,
                num_params: 0,
            }],
        );
        let vm_ptr = &mut vm as *mut crate::vm::VM as *mut ();
        crate::jit::dispatch::elle_jit_push_region_map(vm_ptr);
        let spilled = [Value::int(42)];
        let result = elle_jit_fuel_pause(
            spilled.as_ptr(),
            0,
            vm_ptr as u64,
            closure_val.tag,
            closure_val.payload,
        );
        crate::jit::dispatch::elle_jit_pop_region_map(vm_ptr);
        assert_eq!(result, YIELD_SENTINEL);

        let frames = vm.fiber.suspended.take().expect("the pause parks a frame");
        vm.fiber.signal = None;
        let bits = vm.resume_suspended(frames, Value::int(99));
        assert!(bits.is_empty(), "the resumed frame returns normally");
        let (_, value) = vm.fiber.signal.take().expect("the return sets a value");
        assert_eq!(value.as_int(), Some(42));
    });
}
