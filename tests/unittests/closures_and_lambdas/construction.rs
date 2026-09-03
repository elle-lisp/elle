use super::*;

// ============================================================================
// SECTION 1: Closure Construction and Type Tests
// ============================================================================

#[test]
fn test_closure_type_identification() {
    // Verify closures are properly typed
    let h = elle::primitives::ctx::TestHeap::new();
    let closure = Closure {
        template: template(h.heap(), TemplateProto::new(
            vec![],
            Arity::Exact(0),
            vec![],
        )),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    };
    let value = h.ctx().closure(closure);

    match value {
        v if v.is_closure() => {} // Success
        _ => panic!("Value should be a Closure"),
    }
}

#[test]
fn test_closure_display() {
    // Closures should have a reasonable string representation
    let h = elle::primitives::ctx::TestHeap::new();
    let closure = Closure {
        template: template(h.heap(), TemplateProto {
            num_locals: 1,
            ..TemplateProto::new(vec![], Arity::Exact(1), vec![]) }),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    };
    let value = h.ctx().closure(closure);
    let s = format!("{}", value);
    assert_eq!(s, "<closure>");
}

#[test]
fn test_closure_clone() {
    // Closures should be cloneable
    let h = elle::primitives::ctx::TestHeap::new();
    let mut rt = Runtime::without_stdlib();
    let env = elle::value::arena::alloc_region_slice::<Value>(rt.heap(), &[Value::int(42)]);
    let closure = Closure {
        template: template(h.heap(), TemplateProto {
            num_locals: 2,
            ..TemplateProto::new(vec![1, 2, 3], Arity::Exact(2), vec![]) }),
        env,
        squelch_mask: SignalBits::EMPTY,
    };
    let value1 = h.ctx().closure(closure.clone());
    let value2 = value1;

    // Both should be closures
    assert!(value1.is_closure());
    assert!(value2.is_closure());
}

// ============================================================================
// SECTION 2: Arity Tests
// ============================================================================

#[test]
fn test_arity_exact() {
    let arity = Arity::Exact(3);
    assert!(arity.matches(3));
    assert!(!arity.matches(2));
    assert!(!arity.matches(4));
}

#[test]
fn test_arity_at_least() {
    let arity = Arity::AtLeast(2);
    assert!(!arity.matches(1));
    assert!(arity.matches(2));
    assert!(arity.matches(3));
    assert!(arity.matches(100));
}

#[test]
fn test_arity_range() {
    let arity = Arity::Range(2, 5);
    assert!(!arity.matches(1));
    assert!(arity.matches(2));
    assert!(arity.matches(3));
    assert!(arity.matches(4));
    assert!(arity.matches(5));
    assert!(!arity.matches(6));
}

#[test]
fn test_arity_zero() {
    let arity = Arity::Exact(0);
    assert!(arity.matches(0));
    assert!(!arity.matches(1));
}

// ============================================================================
// SECTION 3: Closure Environment Capture
// ============================================================================

#[test]
fn test_closure_empty_environment() {
    let h = elle::primitives::ctx::TestHeap::new();
    // Closure with no captured variables
    let closure = Closure {
        template: template(h.heap(), TemplateProto::new(
            vec![],
            Arity::Exact(0),
            vec![],
        )),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    };
    assert_eq!(closure.env.len(), 0);
}

#[test]
fn test_closure_single_captured_variable() {
    // Closure capturing one variable
    let mut rt = Runtime::without_stdlib();
    let captured = vec![Value::int(42)];
    let env = elle::value::arena::alloc_region_slice::<Value>(rt.heap(), &captured);
    let closure = Closure {
        template: template(rt.heap(), TemplateProto {
            num_locals: 1,
            ..TemplateProto::new(vec![], Arity::Exact(1), vec![]) }),
        env,
        squelch_mask: SignalBits::EMPTY,
    };
    assert_eq!(closure.env.len(), 1);
    assert_eq!(closure.env[0], Value::int(42));
}

#[test]
fn test_closure_multiple_captured_variables() {
    // Closure capturing multiple variables
    let h = elle::primitives::ctx::TestHeap::new();
    let mut rt = Runtime::without_stdlib();
    let captured = vec![
        Value::int(1),
        Value::int(2),
        h.ctx().string("test"),
        Value::bool(true),
    ];
    let env = elle::value::arena::alloc_region_slice::<Value>(rt.heap(), &captured);
    let closure = Closure {
        template: template(h.heap(), TemplateProto {
            num_locals: 2,
            ..TemplateProto::new(vec![], Arity::Exact(2), vec![]) }),
        env,
        squelch_mask: SignalBits::EMPTY,
    };
    assert_eq!(closure.env.len(), 4);
    assert_eq!(closure.env[0], Value::int(1));
    assert_eq!(closure.env[2], h.ctx().string("test"));
}

#[test]
fn test_closure_environment_sharing() {
    // Multiple closures can share environment data.
    // With RegionSlice, sharing is the same (ptr,len) pointing into the same
    // arena slice; copying the slice copies the pointer without reallocating.
    let mut rt = Runtime::without_stdlib();
    let shared_env = elle::value::arena::alloc_region_slice::<Value>(
        rt.heap(),
        &[Value::int(100), Value::int(200)],
    );

    let closure1 = Closure {
        template: template(rt.heap(), TemplateProto {
            num_locals: 1,
            ..TemplateProto::new(vec![1], Arity::Exact(1), vec![]) }),
        env: shared_env,
        squelch_mask: SignalBits::EMPTY,
    };

    let closure2 = Closure {
        template: template(rt.heap(), TemplateProto {
            num_locals: 1,
            ..TemplateProto::new(vec![2], Arity::Exact(1), vec![]) }),
        env: shared_env,
        squelch_mask: SignalBits::EMPTY,
    };

    // Both closures share the same environment
    assert_eq!(closure1.env[0], closure2.env[0]);
    assert_eq!(closure1.env.len(), closure2.env.len());
}

// ============================================================================
// SECTION 4: Closure Constants and Bytecode
// ============================================================================

#[test]
fn test_closure_bytecode_storage() {
    let h = elle::primitives::ctx::TestHeap::new();
    // Bytecode should be properly stored and retrievable
    let bytecode = vec![1, 2, 3, 4, 5];
    let closure = Closure {
        template: template(h.heap(), TemplateProto::new(
            bytecode.clone(),
            Arity::Exact(0),
            vec![],
        )),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    };
    assert_eq!(closure.template.bytecode(), bytecode);
}

#[test]
fn test_closure_constants_storage() {
    // Constants should be properly stored
    let h = elle::primitives::ctx::TestHeap::new();
    let constants = vec![Value::int(42), h.ctx().string("hello"), Value::bool(true)];
    let closure = Closure {
        template: template(h.heap(), TemplateProto::new(
            vec![],
            Arity::Exact(0),
            constants.clone(),
        )),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    };
    assert_eq!(closure.template.constants(), constants);
}

#[test]
fn test_closure_num_locals() {
    let h = elle::primitives::ctx::TestHeap::new();
    // num_locals should track local variable count
    for num_locals in 0..10 {
        let closure = Closure {
            template: template(h.heap(), TemplateProto {
                num_locals,
                ..TemplateProto::new(vec![], Arity::Exact(0), vec![]) }),
            env: elle::value::region_slice::RegionSlice::empty(),
            squelch_mask: SignalBits::EMPTY,
        };
        assert_eq!(closure.template.num_locals(), num_locals);
    }
}

// ============================================================================
// SECTION 5: Closure Parameter Binding
// ============================================================================

#[test]
fn test_closure_zero_parameters() {
    let h = elle::primitives::ctx::TestHeap::new();
    let closure = Closure {
        template: template(h.heap(), TemplateProto::new(
            vec![],
            Arity::Exact(0),
            vec![],
        )),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    };
    assert!(closure.template.arity().matches(0));
    assert!(!closure.template.arity().matches(1));
}

#[test]
fn test_closure_single_parameter() {
    let h = elle::primitives::ctx::TestHeap::new();
    let closure = Closure {
        template: template(h.heap(), TemplateProto {
            num_locals: 1,
            ..TemplateProto::new(vec![], Arity::Exact(1), vec![]) }),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    };
    assert!(closure.template.arity().matches(1));
}

#[test]
fn test_closure_multiple_parameters() {
    let h = elle::primitives::ctx::TestHeap::new();
    let closure = Closure {
        template: template(h.heap(), TemplateProto {
            num_locals: 3,
            ..TemplateProto::new(vec![], Arity::Exact(3), vec![]) }),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    };
    assert!(closure.template.arity().matches(3));
    assert!(!closure.template.arity().matches(2));
    assert!(!closure.template.arity().matches(4));
}

#[test]
fn test_closure_variadic_parameters() {
    let h = elle::primitives::ctx::TestHeap::new();
    let closure = Closure {
        template: template(h.heap(), TemplateProto {
            num_locals: 1,
            ..TemplateProto::new(vec![], Arity::AtLeast(1), vec![]) }),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    };
    assert!(closure.template.arity().matches(1));
    assert!(closure.template.arity().matches(2));
    assert!(closure.template.arity().matches(10));
}
