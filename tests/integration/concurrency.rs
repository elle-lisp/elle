use crate::common::eval_source;

#[test]
fn test_spawn_rejects_mutable_table_capture() {
    // Test that spawn rejects closures capturing mutable tables
    let result = eval_source(
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
    let result = eval_source("(spawn +)");

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
    let result = eval_source("(spawn)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("1 argument"));
}

#[test]
fn test_spawn_wrong_arity_two_args() {
    // Test spawn with two arguments
    let result = eval_source("(spawn (fn () 1) 2)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("1 argument"));
}

#[test]
fn test_join_wrong_arity() {
    // Test join with no arguments
    let result = eval_source("(join)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("1 argument"));
}

#[test]
fn test_join_wrong_arity_two_args() {
    // Test join with two arguments
    let result = eval_source("(join 1 2)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("1 argument"));
}

#[test]
fn test_join_invalid_argument() {
    // Test join with invalid argument
    let result = eval_source("(join 42)");
    match result {
        Err(e) => {
            assert!(e.contains("thread handle"));
        }
        Ok(_) => panic!("join should reject non-thread-handles"),
    }
}

#[test]
fn test_sleep_negative_duration() {
    // Test that negative sleep duration returns an error
    let result = eval_source("(time/sleep -1)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("non-negative"));
}

#[test]
fn test_sleep_float_negative() {
    // Test negative float sleep duration
    let result = eval_source("(time/sleep -0.5)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("non-negative"));
}

#[test]
fn test_sleep_non_numeric() {
    // Test sleep with non-numeric argument
    let result = eval_source("(time/sleep \"hello\")");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("number"));
}
