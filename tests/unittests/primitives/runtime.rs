use super::*;

// Disassembly tests
#[cfg(feature = "jit")]
#[test]
fn test_disjit_returns_array_for_pure_closure() {
    let (_vm, mut symbols, meta) = setup();
    let disjit = get_primitive(&meta, &mut symbols, "disjit");

    let mut vm2 = VM::new();
    let mut symbols2 = SymbolTable::new();
    let _signals = register_primitives(&mut vm2, &mut symbols2);
    // Compile-only eval of a pure lambda (no stdlib, no `(eval …)` runtime form),
    // so a fresh per-call CompileCtx (primitives + core + prelude) suffices.
    let mut cctx2 = elle::pipeline::CompileCtx::new();
    let result =
        pipeline_eval("(fn (x) x)", &mut symbols2, &mut vm2, &mut cctx2, "<test>").unwrap();

    let disasm = call_primitive(&disjit, &[result]).unwrap();
    let vec = disasm
        .as_array_mut()
        .expect("disbit should return an array");
    let vec = vec.borrow();
    assert!(!vec.is_empty(), "disbit should return non-empty array");
    for elem in vec.iter() {
        assert!(elem.is_string(), "each element should be a string");
    }
}

#[test]
fn test_disbit_type_error_on_non_closure() {
    let (_vm, mut symbols, meta) = setup();
    let disbit = get_primitive(&meta, &mut symbols, "disbit");
    let result = call_primitive(&disbit, &[Value::int(42)]);
    assert!(result.is_err(), "disbit on non-closure should error");
}

#[test]
fn test_disbit_returns_array_for_pure_closure() {
    let (_vm, mut symbols, meta) = setup();
    let disbit = get_primitive(&meta, &mut symbols, "disbit");

    let mut vm2 = VM::new();
    let mut symbols2 = SymbolTable::new();
    let _signals = register_primitives(&mut vm2, &mut symbols2);
    // Compile-only eval of a pure lambda (no stdlib, no `(eval …)` runtime form),
    // so a fresh per-call CompileCtx (primitives + core + prelude) suffices.
    let mut cctx2 = elle::pipeline::CompileCtx::new();
    let result =
        pipeline_eval("(fn (x) x)", &mut symbols2, &mut vm2, &mut cctx2, "<test>").unwrap();

    let ir = call_primitive(&disbit, &[result]).unwrap();
    if !ir.is_nil() {
        let vec = ir.as_array_mut().expect("disjit should return an array");
        let vec = vec.borrow();
        assert!(!vec.is_empty(), "disjit should return non-empty array");
        for elem in vec.iter() {
            assert!(elem.is_string(), "each element should be a string");
        }
    }
}

#[test]
fn test_disjit_type_error_on_non_closure() {
    let (_vm, mut symbols, meta) = setup();
    let disjit = get_primitive(&meta, &mut symbols, "disjit");
    let result = call_primitive(&disjit, &[Value::int(42)]);
    assert!(result.is_err(), "disjit on non-closure should error");
}

#[test]
fn test_call_count_uncalled_closure() {
    eval_full("(let [f (fn (x) x)] (call-count f))", |r| {
        let result = r.unwrap();
        assert_eq!(
            result.as_int(),
            Some(0),
            "uncalled closure should have 0 calls"
        );
    });
}

#[cfg(feature = "jit")]
#[test]
fn test_call_count_after_calls() {
    eval_full(
        "(let [f (fn (x) x)] (f 1) (f 2) (f 3) (call-count f))",
        |r| {
            let result = r.unwrap();
            assert_eq!(
                result.as_int(),
                Some(3),
                "closure called 3 times should report 3"
            );
        },
    );
}

#[test]
fn test_call_count_non_closure_returns_zero() {
    eval_full("(call-count 42)", |r| {
        let result = r.unwrap();
        assert_eq!(
            result.as_int(),
            Some(0),
            "call-count on non-closure should return 0"
        );
    });
}

#[test]
fn test_global_false_for_builtin() {
    // Primitives are compile-time constants (LoadConst), not globals.
    eval_full("(global? '+)", |r| {
        assert_eq!(r.unwrap(), Value::FALSE, "no globals exist in letrec model");
    });
}

#[test]
fn test_global_false_for_local() {
    // A symbol that's never been defined as a global
    eval_full("(global? 'zzz-nonexistent-symbol)", |r| {
        assert_eq!(
            r.unwrap(),
            Value::FALSE,
            "undefined symbol should not be global"
        );
    });
}

#[test]
fn test_string_to_keyword_returns_keyword() {
    eval_full(r#"(string->keyword "foo")"#, |r| {
        let result = r.unwrap();
        assert!(
            result.is_keyword_named("foo"),
            "string->keyword should return the keyword :foo"
        );
    });
}

#[test]
fn test_string_to_keyword_same_name_same_id() {
    eval_full(
        r#"(= (string->keyword "bar") (string->keyword "bar"))"#,
        |r| {
            assert_eq!(
                r.unwrap(),
                Value::TRUE,
                "same name should produce equal keywords"
            );
        },
    );
}

#[test]
fn test_string_to_keyword_different_names_differ() {
    eval_full(
        r#"(= (string->keyword "aaa") (string->keyword "bbb"))"#,
        |r| {
            assert_eq!(
                r.unwrap(),
                Value::FALSE,
                "different names should produce different keywords"
            );
        },
    );
}

#[test]
fn test_string_to_keyword_type_error_on_non_string() {
    eval_full(r#"(string->keyword 42)"#, |result| {
        assert!(
            result.is_err(),
            "string->keyword on non-string should error"
        );
    });
}

// ============================================================================
// fiber/self (SIG_QUERY)
// ============================================================================

#[test]
fn test_fiber_self_from_root_is_nil() {
    eval_full("(fiber/self)", |r| {
        assert_eq!(r.unwrap(), Value::NIL, "fiber/self from root should be nil");
    });
}

#[test]
fn test_fiber_self_from_fiber_is_fiber() {
    eval_full(
        "(let [f (fiber/new (fn () (fiber/self)) 0)]
           (fiber/resume f nil)
           (fiber/value f))",
        |r| {
            let result = r.unwrap();
            assert!(
                result.as_fiber().is_some(),
                "fiber/self from inside a fiber should return a fiber"
            );
        },
    );
}

#[test]
fn test_fiber_self_identity() {
    // fiber/self should return the same fiber that the parent holds
    eval_full(
        "(let [f (fiber/new (fn () (fiber/self)) 0)]
           (fiber/resume f nil)
            (identical? f (fiber/value f)))",
        |r| {
            assert_eq!(
                r.unwrap(),
                Value::TRUE,
                "fiber/self should be identical? to the fiber handle"
            );
        },
    );
}

// ============================================================================
// doc (SIG_QUERY primitive)
// ============================================================================

#[test]
fn test_doc_returns_string_for_known_primitive() {
    eval_full(r#"(doc "pair")"#, |r| {
        let result = r.unwrap();
        let s = result
            .with_string(|s| s.to_string())
            .expect("doc should return a string");
        assert!(
            s.contains("pair"),
            "doc for pair should contain 'pair', got: {}",
            s
        );
    });
}

#[test]
fn test_doc_returns_not_found_for_unknown() {
    eval_full(r#"(doc "zzz-nonexistent")"#, |r| {
        let result = r.unwrap();
        let s = result
            .with_string(|s| s.to_string())
            .expect("doc should return a string");
        assert!(
            s.contains("No documentation found"),
            "doc for unknown should say not found, got: {}",
            s
        );
    });
}

#[test]
fn test_doc_accepts_keyword() {
    eval_full(r#"(doc (string->keyword "+"))"#, |r| {
        let result = r.unwrap();
        let s = result
            .with_string(|s| s.to_string())
            .expect("doc should return a string");
        assert!(
            s.contains("+"),
            "doc for + via keyword should contain '+', got: {}",
            s
        );
    });
}

#[test]
fn test_doc_wrong_arity() {
    eval_full(r#"(doc "a" "b")"#, |result| {
        assert!(result.is_err(), "doc with 2 args should error");
    });
}

#[test]
fn test_doc_bare_symbol_special_form() {
    eval_full("(doc if)", |r| {
        let result = r.unwrap();
        let s = result
            .with_string(|s| s.to_string())
            .expect("doc should return a string");
        assert!(
            s.contains("Conditional"),
            "doc for if should describe conditional, got: {}",
            s
        );
    });
}

#[test]
fn test_doc_bare_symbol_primitive() {
    eval_full("(doc list)", |r| {
        let result = r.unwrap();
        let s = result
            .with_string(|s| s.to_string())
            .expect("doc should return a string");
        assert!(
            s.contains("list"),
            "doc for list via bare symbol should contain 'list', got: {}",
            s
        );
    });
}

#[test]
fn test_doc_bare_symbol_macro() {
    eval_full("(doc defn)", |r| {
        let result = r.unwrap();
        let s = result
            .with_string(|s| s.to_string())
            .expect("doc should return a string");
        assert!(
            s.contains("defn"),
            "doc for defn should contain 'defn', got: {}",
            s
        );
    });
}

#[test]
fn test_fn_predicate() {
    // fn? is true for both closures and native functions
    assert_eq!(run("(fn? +)"), "true");
    assert_eq!(run("(fn? (fn [x] x))"), "true");
    assert_eq!(run("(fn? 42)"), "false");
    assert_eq!(run("(fn? nil)"), "false");
}

#[test]
fn test_native_fn_predicate() {
    assert_eq!(run("(native-fn? list)"), "true");
    assert_eq!(run("(native-fn? (fn [x] x))"), "false");
    assert_eq!(run("(native-fn? 42)"), "false");
}

#[test]
fn test_native_fn_aliases() {
    assert_eq!(run("(native? list)"), "true");
    assert_eq!(run("(primitive? list)"), "true");
    assert_eq!(run("(native? (fn [x] x))"), "false");
}

#[test]
fn test_type_of_native_fn() {
    assert_eq!(run("(type-of list)"), ":native-fn");
}
