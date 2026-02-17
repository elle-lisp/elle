// Integration tests for custom type handler API.

use elle::ffi::handlers::{HandlerRegistry, TypeHandler, TypeId};
use elle::ffi::marshal::CValue;
use elle::ffi::types::CType;
use elle::value::Value;
use std::sync::Arc;

/// Simple test handler for integers.
struct IntHandler;

impl TypeHandler for IntHandler {
    fn elle_to_c(&self, value: &Value, _ctype: &CType) -> Result<CValue, String> {
        if let Some(n) = value.as_int() {
            Ok(CValue::Int(n))
        } else {
            Err("IntHandler: expected integer".to_string())
        }
    }

    fn c_to_elle(&self, cval: &CValue, _ctype: &CType) -> Result<Value, String> {
        match cval {
            CValue::Int(n) => Ok(Value::int(*n)),
            _ => Err("IntHandler: expected integer CValue".to_string()),
        }
    }

    fn can_handle(&self, _ctype: &CType) -> bool {
        true
    }

    fn priority(&self) -> i32 {
        10
    }
}

/// Flexible handler with custom priority.
struct CustomHandler {
    priority: i32,
}

impl TypeHandler for CustomHandler {
    fn elle_to_c(&self, _value: &Value, _ctype: &CType) -> Result<CValue, String> {
        Ok(CValue::Int(42))
    }

    fn c_to_elle(&self, _cval: &CValue, _ctype: &CType) -> Result<Value, String> {
        Ok(Value::int(42))
    }

    fn can_handle(&self, _ctype: &CType) -> bool {
        true
    }

    fn priority(&self) -> i32 {
        self.priority
    }
}

#[test]
fn test_handler_registration() {
    let registry = HandlerRegistry::new();
    let type_id = TypeId::new("TestInt");
    let handler = Arc::new(IntHandler);

    assert!(registry.register(type_id.clone(), handler).is_ok());
    assert!(registry.has_handler(&type_id).unwrap());
}

#[test]
fn test_handler_unregistration() {
    let registry = HandlerRegistry::new();
    let type_id = TypeId::new("TestInt");
    let handler = Arc::new(IntHandler);

    registry.register(type_id.clone(), handler).unwrap();
    assert!(registry.has_handler(&type_id).unwrap());

    registry.unregister(&type_id).unwrap();
    assert!(!registry.has_handler(&type_id).unwrap());
}

#[test]
fn test_handler_lookup() {
    let registry = HandlerRegistry::new();
    let type_id = TypeId::new("MyInt");
    let handler = Arc::new(IntHandler);

    let _ = registry.register(type_id.clone(), handler);

    let found = registry.get(&type_id);
    assert!(found.is_some());
}

#[test]
fn test_handler_priority() {
    let registry = HandlerRegistry::new();
    let type_id1 = TypeId::new("Low");
    let type_id2 = TypeId::new("High");

    let handler1 = Arc::new(CustomHandler { priority: 5 });
    let handler2 = Arc::new(CustomHandler { priority: 20 });

    registry.register(type_id1.clone(), handler1).unwrap();
    registry.register(type_id2.clone(), handler2).unwrap();

    let (found_id, _) = registry.find_handler(&CType::Int).unwrap();
    assert_eq!(found_id.name(), "High");
}

#[test]
fn test_handler_can_handle() {
    let registry = HandlerRegistry::new();
    let type_id = TypeId::new("Generic");
    let handler = Arc::new(IntHandler);

    registry.register(type_id, handler).unwrap();

    // Find handler should succeed for any type since IntHandler::can_handle always returns true
    let result = registry.find_handler(&CType::Int);
    assert!(result.is_some());

    let result = registry.find_handler(&CType::Float);
    assert!(result.is_some());

    let result = registry.find_handler(&CType::Double);
    assert!(result.is_some());
}

#[test]
fn test_handler_metadata() {
    let registry = HandlerRegistry::new();
    let type_id = TypeId::new("WithMetadata");
    let handler = Arc::new(IntHandler);

    registry.register(type_id.clone(), handler).unwrap();

    let metadata = registry.get_metadata(&type_id).unwrap();
    assert_eq!(metadata.type_id.name(), "WithMetadata");
    assert_eq!(metadata.priority, 10);
}

#[test]
fn test_list_handlers() {
    let registry = HandlerRegistry::new();
    let type_id1 = TypeId::new("Handler1");
    let type_id2 = TypeId::new("Handler2");

    let handler1 = Arc::new(IntHandler);
    let handler2 = Arc::new(CustomHandler { priority: 5 });

    registry.register(type_id1, handler1).unwrap();
    registry.register(type_id2, handler2).unwrap();

    let handlers = registry.list_handlers().unwrap();
    assert_eq!(handlers.len(), 2);
}

#[test]
fn test_clear_handlers() {
    let registry = HandlerRegistry::new();
    let type_id1 = TypeId::new("Handler1");
    let type_id2 = TypeId::new("Handler2");

    let handler1 = Arc::new(IntHandler);
    let handler2 = Arc::new(IntHandler);

    registry.register(type_id1.clone(), handler1).unwrap();
    registry.register(type_id2.clone(), handler2).unwrap();

    assert_eq!(registry.list_handlers().unwrap().len(), 2);

    registry.clear().unwrap();
    assert_eq!(registry.list_handlers().unwrap().len(), 0);
}

#[test]
fn test_multiple_handlers_different_priorities() {
    let registry = HandlerRegistry::new();

    for priority in [5, 15, 10, 20, 1].iter() {
        let type_id = TypeId::new(format!("Handler{}", priority));
        let handler = Arc::new(CustomHandler {
            priority: *priority,
        });
        registry.register(type_id, handler).unwrap();
    }

    let (found_id, _) = registry.find_handler(&CType::Int).unwrap();
    assert_eq!(found_id.name(), "Handler20");
}

#[test]
fn test_handler_elle_to_c() {
    let registry = HandlerRegistry::new();
    let type_id = TypeId::new("IntConverter");
    let handler = Arc::new(IntHandler);

    registry.register(type_id.clone(), handler.clone()).unwrap();

    let result = handler.elle_to_c(&Value::int(42), &CType::Int);
    assert!(result.is_ok());

    let cval = result.unwrap();
    if let CValue::Int(n) = cval {
        assert_eq!(n, 42)
    } else {
        panic!("Expected Int CValue")
    }
}

#[test]
fn test_handler_c_to_elle() {
    let registry = HandlerRegistry::new();
    let type_id = TypeId::new("IntConverter");
    let handler = Arc::new(IntHandler);

    registry.register(type_id.clone(), handler.clone()).unwrap();

    let result = handler.c_to_elle(&CValue::Int(99), &CType::Int);
    assert!(result.is_ok());

    let val = result.unwrap();
    if let Some(n) = val.as_int() {
        assert_eq!(n, 99)
    } else {
        panic!("Expected Int Value")
    }
}

#[test]
fn test_handler_error_handling() {
    let handler = Arc::new(IntHandler);

    // Try to convert wrong type
    let result = handler.elle_to_c(&Value::float(std::f64::consts::PI), &CType::Int);
    assert!(result.is_err());

    let error = result.unwrap_err();
    assert!(error.contains("IntHandler"));
}

#[test]
fn test_registry_clone() {
    let registry1 = HandlerRegistry::new();
    let type_id = TypeId::new("Cloned");
    let handler = Arc::new(IntHandler);

    registry1.register(type_id.clone(), handler).unwrap();

    let registry2 = registry1.clone();
    assert!(registry2.has_handler(&type_id).unwrap());
}

#[test]
fn test_handler_not_found() {
    let registry = HandlerRegistry::new();
    let type_id = TypeId::new("NonExistent");

    let result = registry.get(&type_id);
    assert!(result.is_none());

    let result = registry.unregister(&type_id);
    assert!(result.is_err());
}

#[test]
fn test_empty_registry_find() {
    let registry = HandlerRegistry::new();

    let result = registry.find_handler(&CType::Int);
    assert!(result.is_none());
}

#[test]
fn test_selective_handler_matching() {
    struct SelectiveHandler;

    impl TypeHandler for SelectiveHandler {
        fn elle_to_c(&self, value: &Value, _ctype: &CType) -> Result<CValue, String> {
            if let Some(n) = value.as_int() {
                Ok(CValue::Int(n))
            } else {
                Err("Can only handle integers".to_string())
            }
        }

        fn c_to_elle(&self, cval: &CValue, _ctype: &CType) -> Result<Value, String> {
            match cval {
                CValue::Int(n) => Ok(Value::int(*n)),
                _ => Err("Can only handle integers".to_string()),
            }
        }

        fn can_handle(&self, ctype: &CType) -> bool {
            matches!(ctype, CType::Int | CType::Long)
        }

        fn priority(&self) -> i32 {
            0
        }
    }

    let registry = HandlerRegistry::new();
    let type_id = TypeId::new("Selective");
    let handler = Arc::new(SelectiveHandler);

    registry.register(type_id.clone(), handler).unwrap();

    // Should find handler for Int and Long
    assert!(registry.find_handler(&CType::Int).is_some());
    assert!(registry.find_handler(&CType::Long).is_some());

    // Should not find handler for Float (can_handle returns false)
    // Since there are no other handlers, result is None
    assert!(registry.find_handler(&CType::Float).is_none());
}
