// audited: 2026-11-27
//! The frame `elle_jit_yield` parks from a site that keeps some locals: each local it does not keep reads nil.
//!
//! docs/impl/jit-closure.md
use super::*;
use crate::lir::code::LiveSlots;

/// The buffer is the one every exit of the function shares, so the slot of a
/// local this site does not keep holds whatever an earlier exit stored there.
/// Here that is 999, a value no local holds at this site.
///
/// The counter-factual is a rebuild that reads every local's slot: the
/// parked frame then holds 999 for local 1, and where an earlier exit's value
/// was a heap value the release table names, the walk over an abandoned
/// frame releases it a second time.
#[test]
fn a_local_the_site_does_not_keep_parks_as_nil() {
    crate::value::arena::with_test_region(|| {
        let layout = SpillLayout::default()
            .locals(3)
            .operands(1)
            .kept([0, 2].into_iter().collect::<LiveSlots>());
        let (mut vm, closure_val) = setup_yield_test(
            vec![0x01],
            vec![],
            vec![],
            vec![YieldPointMeta::new(4, layout)],
        );

        // Spilled: [local0, local1 (stale), local2, op0]
        let spilled = [
            Value::int(100),
            Value::int(999),
            Value::int(300),
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
            SIG_YIELD.raw(),
        );

        let frame = as_bytecode_frame(&vm.fiber.suspended.as_ref().unwrap()[0]);
        let stack: Vec<Option<i64>> = frame.stack.iter().map(|v| v.as_int()).collect();
        assert_eq!(stack, vec![Some(100), None, Some(300), Some(40)]);
        assert!(frame.stack[1].is_nil(), "a local the site drops reads nil");
    });
}

/// A celled local the site does not keep leaves nil in its env slot, where
/// a kept one leaves its cell.
#[test]
fn a_celled_local_the_site_does_not_keep_parks_as_nil() {
    crate::value::arena::with_test_region(|| {
        // No params; the mask cells locals 0 and 1, and the site keeps 0.
        let layout = SpillLayout::default()
            .locals(2)
            .kept([0].into_iter().collect::<LiveSlots>());
        let (mut vm, closure_val) = setup_yield_test_with_lbox(
            vec![0x02],
            vec![],
            vec![],
            vec![YieldPointMeta::new(6, layout)],
            0,
            0,
            0b11,
        );

        // An integer stands in for each cell: the layout is the claim.
        let spilled = [Value::int(21), Value::int(999)];
        elle_jit_yield(
            Value::int(0).tag,
            Value::int(0).payload,
            spilled.as_ptr(),
            0,
            &mut vm as *mut crate::vm::VM as *mut () as u64,
            closure_val.tag,
            closure_val.payload,
            SIG_YIELD.raw(),
        );

        let frame = as_bytecode_frame(&vm.fiber.suspended.as_ref().unwrap()[0]);
        let env: Vec<Option<i64>> = frame.env.iter().map(|v| v.as_int()).collect();
        assert_eq!(env, vec![Some(21), None]);
        assert!(
            frame.stack.iter().all(|v| v.is_nil()),
            "a celled local's stack slot is nil"
        );
    });
}
