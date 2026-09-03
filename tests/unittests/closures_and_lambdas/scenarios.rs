use super::*;

// ============================================================================
// SECTION 6: Closure Equality and Hashing
// ============================================================================

#[test]
fn test_closures_never_equal() {
    // Closures should never compare equal (even with identical contents)
    let h = elle::primitives::ctx::TestHeap::new();
    let closure1 = h.ctx().closure(Closure {
        template: template(h.heap(), TemplateProto::new(
            vec![],
            Arity::Exact(0),
            vec![],
        )),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    });

    let closure2 = h.ctx().closure(Closure {
        template: template(h.heap(), TemplateProto::new(
            vec![],
            Arity::Exact(0),
            vec![],
        )),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    });

    // Even though they're structurally identical, they should not be equal
    assert!(closure1 != closure2);
}

#[test]
fn test_same_closure_reference_equality() {
    // Same closure reference should be equal via Rc
    let h = elle::primitives::ctx::TestHeap::new();
    let closure_rc = Rc::new(Closure {
        template: template(h.heap(), TemplateProto::new(
            vec![],
            Arity::Exact(0),
            vec![],
        )),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    });

    let h = elle::primitives::ctx::TestHeap::new();
    let value1 = h.ctx().closure((*closure_rc).clone());
    let value2 = h.ctx().closure((*closure_rc).clone());

    // They're different Value enums even though they wrap the same Rc
    assert!(value1 != value2);
}

// ============================================================================
// SECTION 7: Complex Closure Scenarios
// ============================================================================

#[test]
fn test_closure_with_nested_captured_values() {
    // Closure capturing nested data structures
    let h = elle::primitives::ctx::TestHeap::new();
    let mut rt = Runtime::without_stdlib();
    let nested_list = h
        .ctx()
        .pair(Value::int(1), h.ctx().pair(Value::int(2), Value::NIL));

    let captured = vec![nested_list];
    let env = elle::value::arena::alloc_region_slice::<Value>(rt.heap(), &captured);
    let closure = Closure {
        template: template(h.heap(), TemplateProto::new(
            vec![],
            Arity::Exact(0),
            vec![],
        )),
        env,
        squelch_mask: SignalBits::EMPTY,
    };

    assert_eq!(closure.env.len(), 1);
}

#[test]
fn test_closure_with_closure_in_constants() {
    // A closure's constants can contain other closures
    let h = elle::primitives::ctx::TestHeap::new();
    let inner_closure = h.ctx().closure(Closure {
        template: template(h.heap(), TemplateProto::new(
            vec![1],
            Arity::Exact(0),
            vec![],
        )),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    });

    let outer_closure = Closure {
        template: template(h.heap(), TemplateProto::new(
            vec![],
            Arity::Exact(0),
            vec![inner_closure],
        )),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    };

    assert_eq!(outer_closure.template.constants().len(), 1);
}

#[test]
fn test_closure_with_many_upvalues() {
    // Closure capturing many variables (stress test)
    let mut rt = Runtime::without_stdlib();
    let captured: Vec<Value> = (0..100).map(|i| Value::int(i as i64)).collect();
    let env = elle::value::arena::alloc_region_slice::<Value>(rt.heap(), &captured);

    let closure = Closure {
        template: template(rt.heap(), TemplateProto::new(
            vec![],
            Arity::Exact(0),
            vec![],
        )),
        env,
        squelch_mask: SignalBits::EMPTY,
    };

    assert_eq!(closure.env.len(), 100);
}

// ============================================================================
// SECTION 8: Type Conversions and Accessor Methods
// ============================================================================

#[test]
fn test_closure_as_method() {
    let (mut vm, _symbols) = setup();
    let h = elle::primitives::ctx::TestHeap::new();

    // Priority-1 heap source: the VM already in scope.
    let env = elle::value::arena::alloc_region_slice::<Value>(vm.heap(), &[Value::int(10)]);
    let closure = Closure {
        template: template(h.heap(), TemplateProto {
            num_locals: 2,
            ..TemplateProto::new(vec![], Arity::Exact(2), vec![]) }),
        env,
        squelch_mask: SignalBits::EMPTY,
    };

    let value = h.ctx().closure(closure);

    // Should be able to extract as closure
    match value.as_closure() {
        Some(c) => {
            assert_eq!(c.env.len(), 1);
        }
        None => panic!("Should be a closure"),
    }
}

#[test]
fn test_closure_type_check() {
    let h = elle::primitives::ctx::TestHeap::new();
    let closure = h.ctx().closure(Closure {
        template: template(h.heap(), TemplateProto::new(
            vec![],
            Arity::Exact(0),
            vec![],
        )),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    });

    assert!(closure.is_closure());
    assert!(!matches!(closure, v if v.is_nil()));
    assert!(!matches!(closure, v if v.is_int()));
    assert!(!matches!(closure, v if v.as_native_fn().is_some()));
}

// ============================================================================
// SECTION 9: Closure Scope Behavior
// ============================================================================

#[test]
fn test_closure_environment_isolation() {
    // Different closures should have different environments
    let mut rt = Runtime::without_stdlib();
    let env1 = elle::value::arena::alloc_region_slice::<Value>(rt.heap(), &[Value::int(1)]);
    let env2 = elle::value::arena::alloc_region_slice::<Value>(rt.heap(), &[Value::int(2)]);

    let closure1 = Closure {
        template: template(rt.heap(), TemplateProto::new(
            vec![],
            Arity::Exact(0),
            vec![],
        )),
        env: env1,
        squelch_mask: SignalBits::EMPTY,
    };

    let closure2 = Closure {
        template: template(rt.heap(), TemplateProto::new(
            vec![],
            Arity::Exact(0),
            vec![],
        )),
        env: env2,
        squelch_mask: SignalBits::EMPTY,
    };

    assert_ne!(closure1.env[0], closure2.env[0]);
}

#[test]
fn test_closure_local_variables_count() {
    let h = elle::primitives::ctx::TestHeap::new();
    // num_locals should indicate how many local variables are bound in closure
    for locals in 0..20 {
        let closure = Closure {
            template: template(h.heap(), TemplateProto {
                num_locals: locals,
                ..TemplateProto::new(vec![], Arity::Exact(0), vec![]) }),
            env: elle::value::region_slice::RegionSlice::empty(),
            squelch_mask: SignalBits::EMPTY,
        };
        assert_eq!(closure.template.num_locals(), locals);
    }
}

// ============================================================================
// SECTION 10: Edge Cases
// ============================================================================

#[test]
fn test_closure_with_empty_bytecode() {
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
    assert_eq!(closure.template.bytecode().len(), 0);
}

#[test]
fn test_closure_with_large_bytecode() {
    let h = elle::primitives::ctx::TestHeap::new();
    // Large bytecode should be handled correctly
    let large_code: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
    let closure = Closure {
        template: template(h.heap(), TemplateProto::new(
            large_code.clone(),
            Arity::Exact(0),
            vec![],
        )),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    };
    assert_eq!(closure.template.bytecode().len(), 10000);
}

#[test]
fn test_closure_header_keeps_its_blueprint_alive() {
    // A code object's header holds an `Rc` to the blueprint it was materialized
    // from, which is what keeps that blueprint's payload cached and readable
    // (docs/impl/region/template.md). Dropping every other handle to the
    // blueprint must therefore leave the header's bytecode intact.
    let h = elle::primitives::ctx::TestHeap::new();
    let proto = Rc::new(TemplateProto::new(
        vec![1, 2, 3],
        Arity::Exact(0),
        vec![],
    ));
    let weak = Rc::downgrade(&proto);

    let region = h.heap().new_runtime_region();
    let closure = Closure {
        template: TemplateRef::region(elle::value::closure::materialize(
            h.heap(),
            &proto,
            region,
        )),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    };

    let value = h.ctx().closure(closure);
    drop(proto);

    assert!(
        weak.upgrade().is_some(),
        "the header holds the blueprint, so dropping the builder's handle must \
         not free it"
    );
    assert_eq!(
        value
            .as_closure()
            .expect("a closure value")
            .template
            .bytecode(),
        &[1, 2, 3],
        "the shared payload is still readable through the header"
    );
}

#[test]
fn test_closure_debug_format() {
    let h = elle::primitives::ctx::TestHeap::new();
    let mut rt = Runtime::without_stdlib();
    let env = elle::value::arena::alloc_region_slice::<Value>(rt.heap(), &[Value::int(42)]);
    let closure = Closure {
        template: template(h.heap(), TemplateProto {
            num_locals: 2,
            ..TemplateProto::new(
                vec![1, 2, 3],
                Arity::Exact(2),
                vec![h.ctx().string("test")],
            ) }),
        env,
        squelch_mask: SignalBits::EMPTY,
    };

    let debug_str = format!("{:?}", closure);
    assert!(debug_str.contains("Closure"));
    assert!(debug_str.contains("bytecode"));
    assert!(debug_str.contains("arity"));
}
