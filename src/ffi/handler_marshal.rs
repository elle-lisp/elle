//! Handler-aware marshaling for custom types.
//!
//! This module integrates custom type handlers with the marshaling system,
//! enabling automatic dispatch to custom handlers during type conversion.

use super::handlers::HandlerRegistry;
use super::marshal::{CValue, Marshal};
use super::types::CType;
use crate::value::Value;

/// Handler lookup cache for performance optimization.
#[derive(Clone, Default)]
pub struct HandlerCache {
    // Placeholder for future caching implementation
    // Could cache TypeId -> (handler, priority) mappings
}

impl HandlerCache {
    /// Create a new handler cache.
    pub fn new() -> Self {
        HandlerCache::default()
    }

    /// Clear the cache.
    pub fn clear(&self) {
        // Placeholder for cache clearing
    }
}

/// Marshaling with handler support.
pub struct HandlerMarshal;

impl HandlerMarshal {
    /// Convert an Elle value to C representation, checking for custom handlers first.
    ///
    /// # Arguments
    /// * `value` - The Elle value to marshal
    /// * `ctype` - The target C type
    /// * `registry` - The handler registry to check for custom handlers
    ///
    /// # Returns
    /// * `Ok(CValue)` - Successfully marshaled value
    /// * `Err(String)` - Error message explaining the failure
    pub fn elle_to_c_with_handlers(
        value: &Value,
        ctype: &CType,
        registry: &HandlerRegistry,
    ) -> Result<CValue, String> {
        // Check if there's a handler for this type
        if let Some((_type_id, handler)) = registry.find_handler(ctype) {
            // Try the custom handler first
            match handler.elle_to_c(value, ctype) {
                Ok(cval) => return Ok(cval),
                Err(_) => {
                    // If handler fails, fall through to default marshaling
                    // Log or handle the failure if needed
                }
            }
        }

        // Fall back to default marshaling
        Marshal::elle_to_c(value, ctype)
    }

    /// Convert a C value back to Elle representation, checking for custom handlers first.
    ///
    /// # Arguments
    /// * `cval` - The C value to unmarshal
    /// * `ctype` - The C type information
    /// * `registry` - The handler registry to check for custom handlers
    ///
    /// # Returns
    /// * `Ok(Value)` - Successfully unmarshaled value
    /// * `Err(String)` - Error message explaining the failure
    pub fn c_to_elle_with_handlers(
        cval: &CValue,
        ctype: &CType,
        registry: &HandlerRegistry,
    ) -> Result<Value, String> {
        // Check if there's a handler for this type
        if let Some((_type_id, handler)) = registry.find_handler(ctype) {
            // Try the custom handler first
            match handler.c_to_elle(cval, ctype) {
                Ok(val) => return Ok(val),
                Err(_) => {
                    // If handler fails, fall through to default marshaling
                    // Log or handle the failure if needed
                }
            }
        }

        // Fall back to default marshaling
        Marshal::c_to_elle(cval, ctype)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Test handler that always succeeds
    struct TestHandler;

    impl super::super::handlers::TypeHandler for TestHandler {
        fn elle_to_c(&self, value: &Value, _ctype: &CType) -> Result<CValue, String> {
            match value {
                Value::Int(n) => Ok(CValue::Int(*n)),
                _ => Err("TestHandler: expected integer".to_string()),
            }
        }

        fn c_to_elle(&self, cval: &CValue, _ctype: &CType) -> Result<Value, String> {
            match cval {
                CValue::Int(n) => Ok(Value::Int(*n)),
                _ => Err("TestHandler: expected int CValue".to_string()),
            }
        }

        fn can_handle(&self, _ctype: &CType) -> bool {
            true
        }
    }

    #[test]
    fn test_handler_marshal_fallback_to_default() {
        let registry = HandlerRegistry::new();

        // With no handlers registered, should use default marshaling
        let result =
            HandlerMarshal::elle_to_c_with_handlers(&Value::Int(42), &CType::Int, &registry);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), CValue::Int(42));
    }

    #[test]
    fn test_handler_marshal_with_registered_handler() {
        let registry = HandlerRegistry::new();
        let type_id = super::super::handlers::TypeId::new("TestInt");
        let handler = Arc::new(TestHandler);

        registry.register(type_id, handler).unwrap();

        // With handler registered, should use custom handler
        let result =
            HandlerMarshal::elle_to_c_with_handlers(&Value::Int(99), &CType::Int, &registry);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), CValue::Int(99));
    }

    #[test]
    fn test_handler_c_to_elle_with_handlers() {
        let registry = HandlerRegistry::new();
        let type_id = super::super::handlers::TypeId::new("TestInt");
        let handler = Arc::new(TestHandler);

        registry.register(type_id, handler).unwrap();

        // Test unmarshal with handler
        let result =
            HandlerMarshal::c_to_elle_with_handlers(&CValue::Int(55), &CType::Int, &registry);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::Int(55));
    }

    #[test]
    fn test_handler_cache_creation() {
        let cache = HandlerCache::new();
        cache.clear(); // Should not panic
    }
}
