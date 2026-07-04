use super::*;

// === Fiber integration tests ===

#[test]
fn test_fiber_new_and_status() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        r#"(let [f (fiber/new (fn () 42) 0)]
             (= (fiber/status f) :new))"#,
        symbols,
        vm,
        cctx,
        "<test>",
    );
    match result {
        Ok(v) => assert_eq!(v, Value::bool(true)),
        Err(e) => panic!("Expected Ok(true), got Err: {}", e),
    }
}

#[test]
fn test_fiber_resume_simple() {
    // A fiber that just returns a value
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        r#"(let [f (fiber/new (fn () 42) 0)]
             (fiber/resume f))"#,
        symbols,
        vm,
        cctx,
        "<test>",
    );
    match result {
        Ok(v) => assert_eq!(v, Value::int(42)),
        Err(e) => panic!("Expected Ok(42), got Err: {}", e),
    }
}

#[test]
fn test_fiber_resume_dead_status() {
    // After a fiber completes, its status should be :dead
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        r#"(let [f (fiber/new (fn () 42) 0)]
             (fiber/resume f)
             (= (fiber/status f) :dead))"#,
        symbols,
        vm,
        cctx,
        "<test>",
    );
    match result {
        Ok(v) => assert_eq!(v, Value::bool(true)),
        Err(e) => panic!("Expected Ok(true), got Err: {}", e),
    }
}

#[test]
fn test_fiber_emit_and_resume() {
    // A fiber that emits, then is resumed to completion
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    // SIG_YIELD = 2, mask catches it
    let result = eval(
        r#"(let [f (fiber/new (fn () (emit 2 99) 42) 2)]
             (fiber/resume f)
             (fiber/value f))"#,
        symbols,
        vm,
        cctx,
        "<test>",
    );
    match result {
        Ok(v) => assert_eq!(v, Value::int(99)),
        Err(e) => panic!("Expected Ok(99), got Err: {}", e),
    }
}

#[test]
fn test_fiber_emit_resume_continues() {
    // Resume after emit should continue execution and return final value
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        r#"(let [f (fiber/new (fn () (emit 2 99) 42) 2)]
             (fiber/resume f)
             (fiber/resume f))"#,
        symbols,
        vm,
        cctx,
        "<test>",
    );
    match result {
        Ok(v) => assert_eq!(v, Value::int(42)),
        Err(e) => panic!("Expected Ok(42), got Err: {}", e),
    }
}

#[test]
fn test_fiber_is_fiber() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        r#"(fiber? (fiber/new (fn () 42) 0))"#,
        symbols,
        vm,
        cctx,
        "<test>",
    );
    match result {
        Ok(v) => assert_eq!(v, Value::bool(true)),
        Err(e) => panic!("Expected Ok(true), got Err: {}", e),
    }
}

#[test]
fn test_fiber_not_fiber() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(r#"(fiber? 42)"#, symbols, vm, cctx, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::bool(false)),
        Err(e) => panic!("Expected Ok(false), got Err: {}", e),
    }
}

#[test]
fn test_fiber_emit_through_nested_call() {
    // A fiber whose body calls a function that emits.
    // This tests yield propagation through nested calls.
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        r#"(begin
             (defn inner () (emit 2 99))
             (let [f (fiber/new (fn () (inner) 42) 2)]
               (fiber/resume f)
               (fiber/value f)))"#,
        symbols,
        vm,
        cctx,
        "<test>",
    );
    match result {
        Ok(v) => assert_eq!(v, Value::int(99)),
        Err(e) => panic!("Expected Ok(99), got Err: {}", e),
    }
}

#[test]
fn test_fiber_mask() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        r#"(fiber/mask (fiber/new (fn () 42) 3))"#,
        symbols,
        vm,
        cctx,
        "<test>",
    );
    match result {
        Ok(v) => assert_eq!(v, Value::int(3)),
        Err(e) => panic!("Expected Ok(3), got Err: {}", e),
    }
}
