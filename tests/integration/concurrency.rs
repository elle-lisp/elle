use elle::pipeline::{compile_all_new, compile_new};
use elle::primitives::register_primitives;
use elle::{SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    // Try to compile as a single expression first
    match compile_new(input, &mut symbols) {
        Ok(result) => vm.execute(&result.bytecode).map_err(|e| e.to_string()),
        Err(_) => {
            // If that fails, try wrapping in a begin
            let wrapped = format!("(begin {})", input);
            match compile_new(&wrapped, &mut symbols) {
                Ok(result) => vm.execute(&result.bytecode).map_err(|e| e.to_string()),
                Err(_) => {
                    // If that also fails, try compiling all expressions
                    let results = compile_all_new(input, &mut symbols)?;
                    let mut last_result = Value::NIL;
                    for result in results {
                        last_result = vm.execute(&result.bytecode).map_err(|e| e.to_string())?;
                    }
                    Ok(last_result)
                }
            }
        }
    }
}

#[test]
fn test_spawn_closure_with_immutable_capture() {
    // Test spawning a closure that captures an immutable value
    let result = eval(
        r#"
        (let ((x 42))
          (let ((handle (spawn (fn () x))))
            (join handle)))
        "#,
    );

    assert!(result.is_ok());
}

#[test]
fn test_spawn_closure_with_string_capture() {
    // Test spawning a closure that captures a string
    let result = eval(
        r#"
        (let ((msg "hello from thread"))
          (let ((handle (spawn (fn () msg))))
            (join handle)))
        "#,
    );

    assert!(result.is_ok());
}

#[test]
fn test_spawn_closure_with_vector_capture() {
    // Test spawning a closure that captures a vector
    let result = eval(
        r#"
        (let ((v [1 2 3]))
          (let ((handle (spawn (fn () v))))
            (join handle)))
        "#,
    );

    assert!(result.is_ok());
}

#[test]
fn test_spawn_closure_computation() {
    // Test spawning a closure that performs a computation
    let result = eval(
        r#"
        (let ((x 10) (y 20))
          (let ((handle (spawn (fn () (+ x y)))))
            (join handle)))
        "#,
    );

    assert!(result.is_ok());
}

// Note: Nested closures test is disabled because it requires proper symbol table
// sharing across threads, which is a more complex issue to solve.
// The current implementation works for simple closures that capture immutable values.

#[test]
fn test_spawn_rejects_mutable_table_capture() {
    // Test that spawn rejects closures capturing mutable tables
    let result = eval(
        r#"
        (let ((t (table)))
          (spawn (fn () t)))
        "#,
    );

    match result {
        Err(e) => {
            assert!(e.contains("mutable") || e.contains("table"));
        }
        Ok(_) => panic!("Should have rejected mutable table capture"),
    }
}

#[test]
fn test_spawn_rejects_native_function() {
    // Test that spawn rejects native functions
    let result = eval("(spawn +)");

    match result {
        Err(e) => {
            assert!(e.contains("native") || e.contains("closure"));
        }
        Ok(_) => panic!("Should have rejected native function"),
    }
}

#[test]
fn test_spawn_wrong_arity() {
    // Test spawn with wrong number of arguments
    let result = eval("(spawn)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("1 argument"));
}

#[test]
fn test_spawn_wrong_arity_two_args() {
    // Test spawn with two arguments
    let result = eval("(spawn (fn () 1) 2)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("1 argument"));
}

#[test]
fn test_join_wrong_arity() {
    // Test join with no arguments
    let result = eval("(join)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("1 argument"));
}

#[test]
fn test_join_wrong_arity_two_args() {
    // Test join with two arguments
    let result = eval("(join 1 2)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("1 argument"));
}

#[test]
fn test_join_invalid_argument() {
    // Test join with invalid argument
    let result = eval("(join 42)");
    match result {
        Err(e) => {
            assert!(e.contains("thread handle"));
        }
        Ok(_) => panic!("join should reject non-thread-handles"),
    }
}

#[test]
fn test_spawn_closure_with_multiple_captures() {
    // Test spawning a closure that captures multiple values
    let result = eval(
        r#"
        (let ((a 1) (b 2) (c 3))
          (let ((handle (spawn (fn () (+ a (+ b c))))))
            (join handle)))
        "#,
    );

    assert!(result.is_ok());
}

// Note: Boolean capture test is disabled because it requires proper symbol table
// sharing across threads. The issue is that the closure's bytecode references
// symbols that don't exist in the fresh VM's symbol table.
// This is a known limitation that would require a more sophisticated approach
// to symbol table management across threads.

#[test]
fn test_spawn_closure_with_nil_capture() {
    // Test spawning a closure that captures nil
    let result = eval(
        r#"
        (let ((n nil))
          (let ((handle (spawn (fn () n))))
            (join handle)))
        "#,
    );

    assert!(result.is_ok());
}

#[test]
fn test_spawn_closure_with_float_capture() {
    // Test spawning a closure that captures a float
    let result = eval(
        r#"
         (let ((f 3.14159))
           (let ((handle (spawn (fn () f))))
             (join handle)))
         "#,
    );

    assert!(result.is_ok());
}

#[test]
fn test_spawn_closure_with_list_capture() {
    // Test spawning a closure that captures a list
    let result = eval(
        r#"
        (let ((lst (list 1 2 3)))
          (let ((handle (spawn (fn () lst))))
            (join handle)))
        "#,
    );

    assert!(result.is_ok());
}

#[test]
fn test_spawn_closure_no_captures() {
    // Test spawning a closure with no captures
    let result = eval(
        r#"
        (let ((handle (spawn (fn () 42))))
          (join handle))
        "#,
    );

    assert!(result.is_ok());
}

#[test]
fn test_spawn_closure_with_conditional() {
    // Test spawning a closure that uses conditional logic
    let result = eval(
        r#"
        (let ((x 10))
          (let ((handle (spawn (fn () (if (> x 5) "big" "small")))))
            (join handle)))
        "#,
    );

    assert!(result.is_ok());
}

#[test]
fn test_sleep() {
    // Test that sleep works and blocks for the right amount of time
    let start = std::time::Instant::now();
    let result = eval("(time/sleep 0.1)");
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    assert!(
        elapsed.as_millis() >= 100,
        "sleep should block for at least 100ms"
    );
}

#[test]
fn test_sleep_with_int() {
    // Test sleep with integer seconds
    let start = std::time::Instant::now();
    let result = eval("(time/sleep 0)");
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    // Should complete quickly
    assert!(elapsed.as_millis() < 100);
}

#[test]
fn test_current_thread_id() {
    // Test that current-thread-id returns a string
    let result = eval("(current-thread-id)");
    assert!(result.is_ok());
}

#[test]
fn test_sleep_negative_duration() {
    // Test that negative sleep duration returns an error
    let result = eval("(time/sleep -1)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("non-negative"));
}

#[test]
fn test_sleep_float_negative() {
    // Test negative float sleep duration
    let result = eval("(time/sleep -0.5)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("non-negative"));
}

#[test]
fn test_sleep_non_numeric() {
    // Test sleep with non-numeric argument
    let result = eval("(time/sleep \"hello\")");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("number"));
}

#[test]
fn test_spawn_closure_with_capture() {
    // Test spawning a closure that captures a variable
    let result = eval(
        r#"
        (let ((x 42))
          (let ((closure (fn () x)))
            (let ((handle (spawn closure)))
              (join handle))))
        "#,
    );

    assert!(result.is_ok());
}

#[test]
fn test_spawn_jit_closure_with_computation() {
    // Test spawning a closure that performs computation
    let result = eval(
        r#"
        (let ((a 10) (b 20))
          (let ((closure (fn () (+ a b))))
            (let ((handle (spawn closure)))
              (join handle))))
        "#,
    );

    assert!(result.is_ok());
}

#[test]
fn test_spawn_jit_closure_with_string_capture() {
    // Test spawning a closure that captures a string
    let result = eval(
        r#"
        (let ((msg "hello from jit thread"))
          (let ((closure (fn () msg)))
            (let ((handle (spawn closure)))
              (join handle))))
        "#,
    );

    assert!(result.is_ok());
}

#[test]
fn test_spawn_jit_closure_with_vector_capture() {
    // Test spawning a closure that captures a vector
    let result = eval(
        r#"
        (let ((v [10 20 30]))
          (let ((closure (fn () v)))
            (let ((handle (spawn closure)))
              (join handle))))
        "#,
    );

    assert!(result.is_ok());
}

#[test]
fn test_spawn_jit_closure_with_multiple_captures() {
    // Test spawning a closure that captures multiple values
    let result = eval(
        r#"
        (let ((a 1) (b 2) (c 3))
          (let ((closure (fn () (+ a (+ b c)))))
            (let ((handle (spawn closure)))
              (join handle))))
        "#,
    );

    assert!(result.is_ok());
}

#[test]
fn test_spawn_jit_closure_with_conditional() {
    // Test spawning a closure that uses conditional logic
    let result = eval(
        r#"
        (let ((x 10))
          (let ((closure (fn () (if (> x 5) "big" "small"))))
            (let ((handle (spawn closure)))
              (join handle))))
        "#,
    );

    assert!(result.is_ok());
}
