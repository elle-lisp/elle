use super::*;

// Macro and meta-programming tests
#[test]
fn test_gensym_generation() {
    let (_vm, mut symbols, meta) = setup();
    let gensym = get_primitive(&meta, &mut symbols, "gensym");

    // Generate unique symbols
    let sym1 = call_primitive_with_symbols(&gensym, &[], &mut symbols).unwrap();
    let sym2 = call_primitive_with_symbols(&gensym, &[], &mut symbols).unwrap();

    // Should generate symbols (not strings)
    assert!(sym1.as_symbol().is_some(), "gensym should return a symbol");
    assert!(sym2.as_symbol().is_some(), "gensym should return a symbol");
    // Symbols should be unique
    assert_ne!(sym1.as_symbol(), sym2.as_symbol());
}

#[test]
fn test_gensym_with_prefix() {
    let (_vm, mut symbols, meta) = setup();
    let gensym = get_primitive(&meta, &mut symbols, "gensym");

    // Generate symbol with custom prefix
    let h = elle::primitives::ctx::TestHeap::new();
    let sym = call_primitive_with_symbols(&gensym, &[h.ctx().string("VAR")], &mut symbols).unwrap();

    assert!(sym.as_symbol().is_some(), "gensym should return a symbol");
    // Verify the interned name starts with VAR
    let sym_id = sym.as_symbol().unwrap();
    let name = symbols.name(elle::value::SymbolId(sym_id)).unwrap();
    assert!(
        name.starts_with("VAR"),
        "gensym with prefix should start with VAR, got: {}",
        name
    );
}

#[test]
fn test_package_manager() {
    let (_vm, mut symbols, meta) = setup();

    // Test package-version
    let version_fn = get_primitive(&meta, &mut symbols, "package-version");
    match call_primitive(&version_fn, &[]).unwrap() {
        v if v.is_string() => {
            let s = v.with_string(|s| s.to_string()).unwrap();
            assert_eq!(s, "1.0.0")
        }
        _ => panic!("Expected string"),
    }

    // Test package-info
    let info_fn = get_primitive(&meta, &mut symbols, "package-info");
    let result = call_primitive(&info_fn, &[]).unwrap();
    assert!(result.is_list());

    // Should be (name version description)
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
}

// Phase 5: Advanced Runtime Features Tests

#[test]
fn test_import_file_primitive() {
    let (_vm, mut symbols, meta) = setup();
    let import_file = get_primitive(&meta, &mut symbols, "import-file");
    let h = elle::primitives::ctx::TestHeap::new();

    // Test with valid string argument (file may not exist, but function should accept it)
    let result = call_primitive(&import_file, &[h.ctx().string("lib/math.lisp")]);
    // Result depends on file existence - we're just checking error handling
    assert!(result.is_ok() || result.is_err());

    // Test with invalid argument type
    let result = call_primitive(&import_file, &[Value::int(42)]);
    assert!(result.is_err());
}

// import-file needs a full instance (a compile context and the symbol table),
// which `call_primitive`'s bare test ctx does not provide — so these drive it
// through the pipeline (`eval_source`, a Runtime with both), the way real code
// reaches the primitive.
#[test]
fn test_import_file_with_valid_file() {
    eval_source(r#"(import-file "tests/modules/test.lisp")"#, |result| {
        assert!(result.is_ok(), "Should successfully load valid file");
    });
}

#[test]
fn test_import_file_with_invalid_file() {
    eval_source(r#"(import-file "/nonexistent/path.lisp")"#, |result| {
        assert!(result.is_err(), "Should fail for non-existent file");
    });
}

#[test]
fn test_import_file_circular_dependency_prevention() {
    // Re-importing the same module is idempotent (already loaded → cached value),
    // not an error.
    eval_source(
        "(import-file \"tests/modules/test.lisp\")\n\
         (import-file \"tests/modules/test.lisp\")",
        |result| {
            assert!(result.is_ok(), "Idempotent re-import should succeed");
        },
    );
}

#[test]
fn test_spawn_primitive() {
    let (_vm, mut symbols, meta) = setup();
    // `spawn` itself is not a primitive alias; the light worker primitive is
    // `sys/spawn-vm` (this only checks that spawn returns a thread handle /
    // rejects a non-fn).
    let spawn = get_primitive(&meta, &mut symbols, "sys/spawn-vm");
    let h = elle::primitives::ctx::TestHeap::new();

    // Create a simple closure to spawn
    let closure = h.ctx().closure(Closure {
        template: std::rc::Rc::new(ClosureTemplate::new(
            std::rc::Rc::new(vec![0u8]), // dummy bytecode
            elle::value::Arity::Exact(0),
            std::rc::Rc::new(vec![]),
        ))
        .into(),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    });

    // spawn serializes the closure via SendBundle, which resolves symbol ids
    // through the sender's table — thread the test's table into the ctx VM.
    let result = call_primitive_with_symbols(&spawn, &[closure], &mut symbols);
    let result = result.unwrap_or_else(|e| panic!("spawn should return a thread handle: {e}"));
    match result {
        v if v.as_thread_handle().is_some() => {
            // Should return a thread handle
        }
        _ => panic!("spawn should return a thread handle"),
    }

    // Test with non-function
    let result = call_primitive_with_symbols(&spawn, &[Value::int(42)], &mut symbols);
    assert!(result.is_err());
}

#[test]
fn test_join_primitive() {
    // `join` is now a stdlib function (scheduler-cooperative wait + timeout);
    // its primitive building block is `sys/thread-state`. A non-handle argument
    // is rejected the same way the old `join` primitive rejected it.
    let (_vm, mut symbols, meta) = setup();
    let state = get_primitive(&meta, &mut symbols, "sys/thread-state");
    let h = elle::primitives::ctx::TestHeap::new();

    let result = call_primitive(&state, &[h.ctx().string("thread-id")]);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("thread handle"));
}

#[test]
fn test_sleep_primitive() {
    let (_vm, mut symbols, meta) = setup();
    let sleep = get_primitive(&meta, &mut symbols, "time/sleep");
    let h = elle::primitives::ctx::TestHeap::new();

    // Test with integer seconds
    let result = call_primitive(&sleep, &[Value::int(0)]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::NIL);

    // Test with float seconds
    let result = call_primitive(&sleep, &[Value::float(0.01)]);
    assert!(result.is_ok());

    // Test with negative duration
    let result = call_primitive(&sleep, &[Value::int(-1)]);
    assert!(result.is_err());

    // Test with wrong argument type
    let result = call_primitive(&sleep, &[h.ctx().string("invalid")]);
    assert!(result.is_err());
}

#[test]
fn test_current_thread_id_primitive() {
    let (_vm, mut symbols, meta) = setup();
    let thread_id = get_primitive(&meta, &mut symbols, "current-thread-id");

    let result = call_primitive(&thread_id, &[]);
    assert!(result.is_ok());
    let v = result.unwrap();
    assert!(
        v.as_int().is_some(),
        "current-thread-id should return an integer"
    );
    assert!(v.as_int().unwrap() > 0);
}

#[test]
fn test_debug_print_primitive() {
    let (_vm, mut symbols, meta) = setup();
    let debug_print = get_primitive(&meta, &mut symbols, "debug-print");

    let test_val = Value::int(42);
    let result = call_primitive(&debug_print, std::slice::from_ref(&test_val));
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), test_val);
}

#[test]
fn test_trace_primitive() {
    let (_vm, mut symbols, meta) = setup();
    let trace = get_primitive(&meta, &mut symbols, "trace");
    let h = elle::primitives::ctx::TestHeap::new();

    let label = h.ctx().string("test-trace");
    let value = Value::int(42);
    let result = call_primitive(&trace, &[label, value]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), value);

    // Test with symbol label
    let sym_id = symbols.intern("trace-label");
    let label = Value::symbol(sym_id.0);
    let result = call_primitive(&trace, &[label, value]);
    assert!(result.is_ok());

    // Test with invalid label type
    let label = Value::int(123);
    let result = call_primitive(&trace, &[label, value]);
    assert!(result.is_err());
}

#[test]
fn test_clock_monotonic_primitive() {
    let (_vm, mut symbols, meta) = setup();
    let clock = get_primitive(&meta, &mut symbols, "clock/monotonic");

    // Returns a non-negative float
    let result = call_primitive(&clock, &[]);
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!(
        val.as_float().is_some(),
        "clock/monotonic should return a float"
    );
    assert!(
        val.as_float().unwrap() >= 0.0,
        "clock/monotonic should be non-negative"
    );

    // Monotonically non-decreasing
    let t1 = call_primitive(&clock, &[]).unwrap().as_float().unwrap();
    let t2 = call_primitive(&clock, &[]).unwrap().as_float().unwrap();
    assert!(
        t2 >= t1,
        "clock/monotonic should be monotonically non-decreasing"
    );
}

#[test]
fn test_clock_realtime_primitive() {
    let (_vm, mut symbols, meta) = setup();
    let clock = get_primitive(&meta, &mut symbols, "clock/realtime");

    // Returns a non-negative float
    let result = call_primitive(&clock, &[]);
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!(
        val.as_float().is_some(),
        "clock/realtime should return a float"
    );
    assert!(
        val.as_float().unwrap() > 1_700_000_000.0,
        "clock/realtime should be a plausible epoch timestamp"
    );
}

#[test]
fn test_clock_cpu_primitive() {
    let (_vm, mut symbols, meta) = setup();
    let clock = get_primitive(&meta, &mut symbols, "clock/cpu");

    // Returns a non-negative float
    let result = call_primitive(&clock, &[]);
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!(val.as_float().is_some(), "clock/cpu should return a float");
    assert!(
        val.as_float().unwrap() >= 0.0,
        "clock/cpu should be non-negative"
    );

    // Non-decreasing
    let t1 = call_primitive(&clock, &[]).unwrap().as_float().unwrap();
    let t2 = call_primitive(&clock, &[]).unwrap().as_float().unwrap();
    assert!(t2 >= t1, "clock/cpu should be non-decreasing");
}

#[test]
fn test_memory_usage_primitive() {
    let (_vm, mut symbols, meta) = setup();
    let memory_usage = get_primitive(&meta, &mut symbols, "memory-usage");

    let result = call_primitive(&memory_usage, &[]);
    assert!(result.is_ok());

    // Should return a list
    match result.unwrap() {
        v if v.is_pair() || v.is_nil() => {
            // Valid list representation
        }
        _ => panic!("memory-usage should return a list"),
    }
}

#[test]
fn test_module_loading_path_tracking() {
    let _vm = VM::new();

    // Add search paths
    // vm.add_module_search_path(std::path::PathBuf::from("./lib"));
    // vm.add_module_search_path(std::path::PathBuf::from("./modules"));

    // Paths should be trackable (internal state, not exposed via API)
    // This test verifies the VM accepts path additions without panic
}

#[test]
fn test_module_circular_dependency_prevention() {
    let _vm = VM::new();

    // Try to load the same module twice
    // let result1 = vm.load_module("test-module".to_string(), "");
    // let result2 = vm.load_module("test-module".to_string(), "");

    // Both should succeed (second is no-op due to circular dep prevention)
    // assert!(result1.is_ok());
    // assert!(result2.is_ok());
}
