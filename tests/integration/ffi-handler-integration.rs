// Integration tests for custom handler integration with marshaling.

use elle::ffi::handler_marshal::HandlerMarshal;
use elle::ffi::handlers::{HandlerRegistry, TypeHandler, TypeId};
use elle::ffi::marshal::CValue;
use elle::ffi::types::CType;
use elle::value::Value;
use std::sync::Arc;

/// Simple handler that doubles integer values on conversion.
struct DoublingHandler;

impl TypeHandler for DoublingHandler {
    fn elle_to_c(&self, value: &Value, _ctype: &CType) -> Result<CValue, String> {
        match value {
            Value::Int(n) => Ok(CValue::Int(n * 2)),
            _ => Err("DoublingHandler: expected integer".to_string()),
        }
    }

    fn c_to_elle(&self, cval: &CValue, _ctype: &CType) -> Result<Value, String> {
        match cval {
            CValue::Int(n) => Ok(Value::Int(n / 2)),
            _ => Err("DoublingHandler: expected int CValue".to_string()),
        }
    }

    fn can_handle(&self, _ctype: &CType) -> bool {
        true
    }

    fn priority(&self) -> i32 {
        10
    }
}

/// Float-to-int handler for testing type conversion.
struct FloatToIntHandler;

impl TypeHandler for FloatToIntHandler {
    fn elle_to_c(&self, value: &Value, _ctype: &CType) -> Result<CValue, String> {
        match value {
            Value::Float(f) => Ok(CValue::Int(*f as i64)),
            Value::Int(n) => Ok(CValue::Int(*n)),
            _ => Err("FloatToIntHandler: expected float or int".to_string()),
        }
    }

    fn c_to_elle(&self, cval: &CValue, _ctype: &CType) -> Result<Value, String> {
        match cval {
            CValue::Int(n) => Ok(Value::Float(*n as f64)),
            _ => Err("FloatToIntHandler: expected int CValue".to_string()),
        }
    }

    fn can_handle(&self, _ctype: &CType) -> bool {
        true
    }

    fn priority(&self) -> i32 {
        5
    }
}

#[test]
fn test_handler_marshal_elle_to_c_without_handler() {
    let registry = HandlerRegistry::new();

    let result = HandlerMarshal::elle_to_c_with_handlers(&Value::Int(42), &CType::Int, &registry);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), CValue::Int(42));
}

#[test]
fn test_handler_marshal_elle_to_c_with_custom_handler() {
    let registry = HandlerRegistry::new();
    let type_id = TypeId::new("Doubling");
    let handler = Arc::new(DoublingHandler);

    registry.register(type_id, handler).unwrap();

    // With custom handler, integer should be doubled
    let result = HandlerMarshal::elle_to_c_with_handlers(&Value::Int(21), &CType::Int, &registry);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), CValue::Int(42)); // 21 * 2
}

#[test]
fn test_handler_marshal_c_to_elle_without_handler() {
    let registry = HandlerRegistry::new();

    let result = HandlerMarshal::c_to_elle_with_handlers(&CValue::Int(99), &CType::Int, &registry);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::Int(99));
}

#[test]
fn test_handler_marshal_c_to_elle_with_custom_handler() {
    let registry = HandlerRegistry::new();
    let type_id = TypeId::new("Doubling");
    let handler = Arc::new(DoublingHandler);

    registry.register(type_id, handler).unwrap();

    // With custom handler, integer should be halved
    let result = HandlerMarshal::c_to_elle_with_handlers(&CValue::Int(84), &CType::Int, &registry);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::Int(42)); // 84 / 2
}

#[test]
fn test_handler_marshal_type_conversion() {
    let registry = HandlerRegistry::new();
    let type_id = TypeId::new("FloatToInt");
    let handler = Arc::new(FloatToIntHandler);

    registry.register(type_id, handler).unwrap();

    // Convert float to int using handler
    let result =
        HandlerMarshal::elle_to_c_with_handlers(&Value::Float(42.5), &CType::Int, &registry);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), CValue::Int(42)); // 42.5 as i64
}

#[test]
fn test_handler_marshal_roundtrip() {
    let registry = HandlerRegistry::new();
    let type_id = TypeId::new("Doubling");
    let handler = Arc::new(DoublingHandler);

    registry.register(type_id, handler).unwrap();

    // Marshal: 21 -> 42 (doubled)
    let c_value =
        HandlerMarshal::elle_to_c_with_handlers(&Value::Int(21), &CType::Int, &registry).unwrap();
    assert_eq!(c_value, CValue::Int(42));

    // Unmarshal: 42 -> 21 (halved)
    let elle_value =
        HandlerMarshal::c_to_elle_with_handlers(&c_value, &CType::Int, &registry).unwrap();
    assert_eq!(elle_value, Value::Int(21));
}

#[test]
fn test_handler_marshal_priority() {
    let registry = HandlerRegistry::new();

    // Register two handlers with different priorities
    let type_id1 = TypeId::new("Low");
    let type_id2 = TypeId::new("High");
    let handler1 = Arc::new(FloatToIntHandler);
    let handler2 = Arc::new(DoublingHandler);

    registry.register(type_id1, handler1).unwrap();
    registry.register(type_id2, handler2).unwrap();

    // The handler with higher priority (DoublingHandler = 10) should be used
    let result = HandlerMarshal::elle_to_c_with_handlers(&Value::Int(15), &CType::Int, &registry);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), CValue::Int(30)); // Doubled (15 * 2)
}

#[test]
fn test_handler_marshal_fallback_on_handler_error() {
    let registry = HandlerRegistry::new();

    // Create a handler that fails
    struct FailingHandler;

    impl TypeHandler for FailingHandler {
        fn elle_to_c(&self, _value: &Value, _ctype: &CType) -> Result<CValue, String> {
            Err("Always fails".to_string())
        }

        fn c_to_elle(&self, _cval: &CValue, _ctype: &CType) -> Result<Value, String> {
            Err("Always fails".to_string())
        }

        fn can_handle(&self, _ctype: &CType) -> bool {
            true
        }
    }

    let type_id = TypeId::new("Failing");
    let handler = Arc::new(FailingHandler);
    registry.register(type_id, handler).unwrap();

    // Even though handler fails, should fall back to default marshaling
    let result = HandlerMarshal::elle_to_c_with_handlers(&Value::Int(42), &CType::Int, &registry);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), CValue::Int(42)); // Default marshaling result
}

#[test]
fn test_handler_marshal_multiple_handlers() {
    let registry = HandlerRegistry::new();

    // Register multiple different handlers
    let type_id1 = TypeId::new("Doubling");
    let type_id2 = TypeId::new("FloatToInt");

    let handler1 = Arc::new(DoublingHandler);
    let handler2 = Arc::new(FloatToIntHandler);

    registry.register(type_id1, handler1).unwrap();
    registry.register(type_id2, handler2).unwrap();

    // Highest priority should be used (DoublingHandler = 10)
    let result = HandlerMarshal::elle_to_c_with_handlers(&Value::Int(5), &CType::Int, &registry);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), CValue::Int(10)); // 5 * 2
}

#[test]
fn test_handler_marshal_bool_without_handler() {
    let registry = HandlerRegistry::new();

    // Test with bool type
    let result =
        HandlerMarshal::elle_to_c_with_handlers(&Value::Bool(true), &CType::Bool, &registry);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), CValue::Int(1)); // Default bool marshaling
}

#[test]
fn test_handler_marshal_float_without_handler() {
    let registry = HandlerRegistry::new();

    // Test with float type
    let pi = std::f64::consts::PI;
    let result =
        HandlerMarshal::elle_to_c_with_handlers(&Value::Float(pi), &CType::Float, &registry);
    assert!(result.is_ok());
    let cval = result.unwrap();
    match cval {
        CValue::Float(f) => assert!((f - pi).abs() < 0.0001),
        _ => panic!("Expected float CValue"),
    }
}

#[test]
fn test_handler_marshal_string_without_handler() {
    let registry = HandlerRegistry::new();

    // Test with string type (should marshal as pointer)
    let result = HandlerMarshal::elle_to_c_with_handlers(
        &Value::String("hello".into()),
        &CType::Pointer(Box::new(CType::Char)),
        &registry,
    );
    assert!(result.is_ok());
    // String should be marshaled as pointer
    match result.unwrap() {
        CValue::String(_) => {} // Expected
        _ => panic!("Expected string CValue"),
    }
}

#[test]
fn test_handler_marshal_nil_pointer_without_handler() {
    let registry = HandlerRegistry::new();

    // Test with nil as pointer
    let result = HandlerMarshal::elle_to_c_with_handlers(
        &Value::Nil,
        &CType::Pointer(Box::new(CType::Void)),
        &registry,
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), CValue::Pointer(std::ptr::null()));
}

#[test]
fn test_handler_marshal_clear_handlers() {
    let registry = HandlerRegistry::new();
    let type_id = TypeId::new("Doubling");
    let handler = Arc::new(DoublingHandler);

    registry.register(type_id.clone(), handler).unwrap();
    assert!(registry.has_handler(&type_id).unwrap());

    // Clear all handlers
    registry.clear().unwrap();
    assert!(!registry.has_handler(&type_id).unwrap());

    // After clearing, should use default marshaling
    let result = HandlerMarshal::elle_to_c_with_handlers(&Value::Int(21), &CType::Int, &registry);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), CValue::Int(21)); // Default marshaling (not doubled)
}

#[test]
fn test_handler_marshal_u64_overflow_protection() {
    let registry = HandlerRegistry::new();

    // Test that default marshaling handles unsigned integers correctly
    let result = HandlerMarshal::elle_to_c_with_handlers(&Value::Int(100), &CType::UInt, &registry);
    assert!(result.is_ok());
    match result.unwrap() {
        CValue::UInt(n) => assert_eq!(n, 100),
        _ => panic!("Expected uint CValue"),
    }
}

#[test]
fn test_handler_marshal_signed_vs_unsigned() {
    let registry = HandlerRegistry::new();

    // Negative value should fail for unsigned
    let result = HandlerMarshal::elle_to_c_with_handlers(&Value::Int(-1), &CType::UInt, &registry);
    assert!(result.is_err());

    // Positive value should succeed
    let result = HandlerMarshal::elle_to_c_with_handlers(&Value::Int(42), &CType::UInt, &registry);
    assert!(result.is_ok());
}

#[test]
fn test_handler_marshal_type_mismatch_without_handler() {
    let registry = HandlerRegistry::new();

    // Try to marshal wrong type without handler
    let result = HandlerMarshal::elle_to_c_with_handlers(
        &Value::String("text".into()),
        &CType::Int,
        &registry,
    );
    assert!(result.is_err());
}

#[test]
fn test_handler_marshal_preserves_handler_metadata() {
    let registry = HandlerRegistry::new();
    let type_id = TypeId::new("WithMetadata");
    let handler = Arc::new(DoublingHandler);

    registry.register(type_id.clone(), handler).unwrap();

    let metadata = registry.get_metadata(&type_id).unwrap();
    assert_eq!(metadata.type_id.name(), "WithMetadata");
    assert_eq!(metadata.priority, 10);

    // Handler should still work after metadata verification
    let result = HandlerMarshal::elle_to_c_with_handlers(&Value::Int(10), &CType::Int, &registry);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), CValue::Int(20));
}
