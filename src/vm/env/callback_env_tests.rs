use super::*;
use crate::hir::region::RuntimeRegion;
use crate::value::arena::region_of;
use crate::value::types::Arity;
use crate::value::{Closure, ClosureTemplate, SignalBits, Value};
use std::rc::Rc;

/// A VM with a live "caller" region — the situation a real C→Elle callback
/// fires into (some Elle caller's region exists). The region is increfed so it
/// stays live (rc>0): a per-value env region that wrongly reused it (or a stray
/// decref) would be observable, and a fresh mint can never hand back this id.
/// `populate_env` mints its own per-value region, so no region is passed in.
fn vm_with_ambient() -> (VM, RuntimeRegion) {
    let mut vm = VM::new();
    let other = vm.heap().new_runtime_region();
    vm.heap().incref_region(other);
    (vm, other)
}

fn closure(
    arity: Arity,
    num_params: usize,
    num_locals: usize,
    capture_params_mask: u64,
    capture_locals_mask: u64,
    env: crate::value::region_slice::RegionSlice<Value>,
) -> Rc<Closure> {
    let template = Rc::new(ClosureTemplate {
        num_locals,
        num_params,
        num_captures: env.len(),
        capture_params_mask,
        capture_locals_mask: crate::value::CaptureMask::from_u64(capture_locals_mask),
        ..ClosureTemplate::new(Rc::new(vec![]), arity, Rc::new(vec![]))
    });
    Rc::new(Closure {
        template: crate::value::TemplateRef::new(template),
        env,
        squelch_mask: SignalBits::EMPTY,
    })
}

fn empty_env() -> crate::value::region_slice::RegionSlice<Value> {
    crate::value::region_slice::RegionSlice::empty()
}

// ── Behavioral guards (relocated from src/ffi/callback.rs) ──────────

#[test]
fn exact_arity_env_has_params() {
    let mut vm = VM::new();
    let c = closure(Arity::Exact(2), 2, 2, 0, 0, empty_env());
    let env = vm
        .build_callback_env(&c, &[Value::int(10), Value::int(20)])
        .unwrap();
    // 0 captures + 2 params + 0 locals = 2
    assert_eq!(env.len(), 2);
    assert_eq!(env[0].as_int(), Some(10));
    assert_eq!(env[1].as_int(), Some(20));
}

#[test]
fn captured_upvalue_is_copied_then_params() {
    let (mut vm, _ambient) = vm_with_ambient();
    // 1 captured upvalue (99), 1 param (42), 1 bare local. The env slice is born
    // in its own explicit region.
    let env_region = vm.heap().new_runtime_region();
    let env_slice = crate::value::arena::alloc_region_slice_in_region::<Value>(
        unsafe { &mut *vm.heap_ptr },
        &[Value::int(99)],
        env_region,
    );
    let c = closure(Arity::Exact(1), 1, 2, 0, 0, env_slice);
    let env = vm.build_callback_env(&c, &[Value::int(42)]).unwrap();
    assert_eq!(env.len(), 3);
    assert_eq!(env[0].as_int(), Some(99));
    assert_eq!(env[1].as_int(), Some(42));
}

// ── Counterfactuals: every env value the callback builder mints must land in
// its OWN per-execution region (docs/impl/region/rules.md Rule 6), distinct from
// any other live region — `vm_with_ambient`'s extra region stands in for a live
// caller region the cell must not reuse. The builder unifies on `populate_env` +
// `env_value_region`, so each value is born in its own region.

#[test]
fn capture_cell_param_gets_its_own_region_not_ambient() {
    let (mut vm, ambient) = vm_with_ambient();
    // param 0 is a captured (mutated-by-nested-closure) param → a cell.
    let c = closure(Arity::Exact(1), 1, 1, 0b1, 0, empty_env());
    let env = vm.build_callback_env(&c, &[Value::int(42)]).unwrap();
    let cell = env[0];
    assert!(
        cell.is_capture_cell(),
        "captured param must be wrapped in a capture cell"
    );
    let region = region_of(unsafe { &mut *vm.heap_ptr }, cell)
        .expect("a heap capture cell must have a region");
    assert_ne!(
        region, ambient,
        "callback capture cell commingled into the caller's ambient region \
             (Rule 6) — it must mint its own per-value region like populate_env"
    );
}

#[test]
fn rest_list_conses_get_own_regions_not_ambient() {
    let (mut vm, ambient) = vm_with_ambient();
    // variadic: 0 fixed + a rest slot (num_params = 1, the rest slot).
    let c = closure(Arity::AtLeast(0), 1, 1, 0, 0, empty_env());
    let env = vm
        .build_callback_env(&c, &[Value::int(1), Value::int(2)])
        .unwrap();
    let rest_head = env[0];
    let region = region_of(unsafe { &mut *vm.heap_ptr }, rest_head)
        .expect("a rest-list cons must have a region");
    assert_ne!(
        region, ambient,
        "callback rest-list cons commingled into the caller's ambient region \
             (Rule 6) — each cons must mint its own per-value region"
    );
}

#[test]
fn captured_local_cell_gets_its_own_region_not_ambient() {
    let (mut vm, ambient) = vm_with_ambient();
    // 1 param + 1 captured local (capture_locals_mask bit 0).
    let c = closure(Arity::Exact(1), 1, 2, 0, 0b1, empty_env());
    let env = vm.build_callback_env(&c, &[Value::int(7)]).unwrap();
    let local_cell = env[1];
    assert!(
        local_cell.is_capture_cell(),
        "captured local must be a capture cell"
    );
    let region = region_of(unsafe { &mut *vm.heap_ptr }, local_cell)
        .expect("a heap capture cell must have a region");
    assert_ne!(
        region, ambient,
        "callback captured-local cell commingled into the caller's ambient \
             region (Rule 6) — it must mint its own per-value region"
    );
}

// ── Env-region routing: every env value gets its OWN fresh per-execution
// region (the env builder mints one mortal `RuntimeRegion` per value). A capture
// cell must therefore land in a region distinct from the caller's ambient.

#[test]
fn env_capture_cell_gets_a_fresh_region_not_ambient() {
    let (mut vm, ambient) = vm_with_ambient();
    // param 0 is a captured param → wrapped in a cell whose region we read.
    let c = closure(Arity::Exact(1), 1, 1, 0b1, 0, empty_env());
    let env = vm.build_closure_env(&c, &[Value::int(42)]).unwrap();
    let cell = env[0];
    assert!(cell.is_capture_cell(), "captured param must be a cell");
    let region = region_of(unsafe { &mut *vm.heap_ptr }, cell)
        .expect("a heap capture cell must have a region");
    assert_ne!(
        region, ambient,
        "the env builder must mint a fresh per-value region, never reuse the \
             caller's ambient region (Rule 6)",
    );
}
