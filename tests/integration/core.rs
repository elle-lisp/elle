// DEFENSE: Integration tests that require Rust-specific APIs
//
// Tests that can be expressed in pure Elle have been migrated to
// tests/elle/core.lisp. The tests below remain because they need:
// - Float precision checks with .as_float() and tolerance comparisons
// - Error message substring matching
// - Programmatic test generation (deep nesting, large lists)
// - halt primitive (terminates the VM, can't test in a script)
use crate::common::eval_source;
use elle::Value;

// ============================================================================
// Float precision tests — require .as_float() with tolerance
// ============================================================================

#[test]
fn test_int_float_mixing() {
    if let Some(f) = eval_source("(+ 1 2.5)").unwrap().as_float() {
        assert!((f - 3.5).abs() < 1e-10)
    } else {
        panic!("Expected float")
    }

    if let Some(f) = eval_source("(* 2 3.5)").unwrap().as_float() {
        assert!((f - 7.0).abs() < 1e-10)
    } else {
        panic!("Expected float")
    }
}

#[test]
fn test_min_max_float() {
    if let Some(f) = eval_source("(min 1.5 2 0.5)").unwrap().as_float() {
        assert!((f - 0.5).abs() < 1e-10)
    } else {
        panic!("Expected float")
    }
}

#[test]
fn test_abs_float() {
    if let Some(f) = eval_source("(abs -3.5)").unwrap().as_float() {
        assert!((f - 3.5).abs() < 1e-10)
    } else {
        panic!("Expected float")
    }
}

#[test]
fn test_type_conversions_float() {
    if let Some(f) = eval_source("(float 5)").unwrap().as_float() {
        assert!((f - 5.0).abs() < 1e-10)
    } else {
        panic!("Expected float")
    }
}

#[test]
fn test_sqrt() {
    assert_eq!(eval_source("(sqrt 4)").unwrap(), Value::float(2.0));
    assert_eq!(eval_source("(sqrt 9)").unwrap(), Value::float(3.0));
    if let Some(f) = eval_source("(sqrt 16.0)").unwrap().as_float() {
        assert!((f - 4.0).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }
}

#[test]
fn test_trigonometric() {
    if let Some(f) = eval_source("(sin 0)").unwrap().as_float() {
        assert!(f.abs() < 0.0001)
    } else {
        panic!("Expected float")
    }

    if let Some(f) = eval_source("(cos 0)").unwrap().as_float() {
        assert!((f - 1.0).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }

    if let Some(f) = eval_source("(tan 0)").unwrap().as_float() {
        assert!(f.abs() < 0.0001)
    } else {
        panic!("Expected float")
    }
}

#[test]
fn test_log_functions() {
    if let Some(f) = eval_source("(log 1)").unwrap().as_float() {
        assert!(f.abs() < 0.0001)
    } else {
        panic!("Expected float")
    }

    if let Some(f) = eval_source("(log 8 2)").unwrap().as_float() {
        assert!((f - 3.0).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }
}

#[test]
fn test_exp() {
    if let Some(f) = eval_source("(exp 0)").unwrap().as_float() {
        assert!((f - 1.0).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }

    if let Some(f) = eval_source("(exp 1)").unwrap().as_float() {
        assert!((f - std::f64::consts::E).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }
}

#[test]
fn test_pow() {
    assert_eq!(eval_source("(pow 2 3)").unwrap(), Value::int(8));

    if let Some(f) = eval_source("(pow 2 -1)").unwrap().as_float() {
        assert!((f - 0.5).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }

    if let Some(f) = eval_source("(pow 2.0 3.0)").unwrap().as_float() {
        assert!((f - 8.0).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }
}

#[test]
fn test_math_constants() {
    if let Some(f) = eval_source("(pi)").unwrap().as_float() {
        assert!((f - std::f64::consts::PI).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }

    if let Some(f) = eval_source("(e)").unwrap().as_float() {
        assert!((f - std::f64::consts::E).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }
}

// ============================================================================
// Error message content tests — require Rust string inspection
// ============================================================================

#[test]
fn test_undefined_variable_error_shows_name() {
    // Issue #300: error message should show the variable name, not a SymbolId
    let result = eval_source("nonexistent-foo");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("nonexistent-foo"),
        "Error should contain variable name, got: {}",
        err
    );
    assert!(
        !err.contains("symbol #"),
        "Error should not contain raw SymbolId, got: {}",
        err
    );
}

// ============================================================================
// Programmatic stress tests — require Rust string generation
// ============================================================================

#[test]
fn test_deep_arithmetic() {
    // Test with 50 nested additions — requires programmatic generation
    let mut expr = "1".to_string();
    for _ in 0..50 {
        expr = format!("(+ {} 1)", expr);
    }
    assert_eq!(eval_source(&expr).unwrap(), Value::int(51));
}

// ============================================================================
// halt primitive — terminates the VM, cannot be tested in a script
// ============================================================================

#[test]
fn test_halt_returns_value() {
    let result = eval_source("(halt 42)");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_halt_returns_nil() {
    let result = eval_source("(halt)");
    assert_eq!(result.unwrap(), Value::NIL);
}

#[test]
fn test_halt_stops_execution() {
    let result = eval_source("(begin (halt 1) 2)");
    assert_eq!(result.unwrap(), Value::int(1));
}

#[test]
fn test_halt_in_function() {
    let result = eval_source("(begin (def f (fn () (halt 99))) (f))");
    assert_eq!(result.unwrap(), Value::int(99));
}

#[test]
fn test_halt_with_complex_value() {
    let result = eval_source("(halt (list 1 2 3))");
    let vec = result.unwrap().list_to_vec().unwrap();
    assert_eq!(vec, vec![Value::int(1), Value::int(2), Value::int(3)]);
}
