//! Custom type handler API for user-defined FFI marshaling.
//!
//! This module provides an extensible API allowing users to define custom
//! marshaling logic for application-specific types and complex C library patterns.

use super::marshal::CValue;
use super::types::CType;
use crate::value::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Type identifier for custom handlers.
///
/// Handlers are keyed by a unique name that identifies the custom type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeId {
    name: String,
}

impl TypeId {
    /// Create a new type identifier.
    pub fn new(name: impl Into<String>) -> Self {
        TypeId { name: name.into() }
    }

    /// Get the name of this type.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Trait for implementing custom type handlers.
///
/// Handlers define how to marshal values between Elle and C for custom types.
pub trait TypeHandler: Send + Sync {
    /// Convert an Elle value to C representation.
    ///
    /// # Arguments
    /// * `value` - The Elle value to marshal
    /// * `ctype` - The target C type information
    ///
    /// # Returns
    /// * `Ok(CValue)` - Successfully marshaled value
    /// * `Err(String)` - Error message explaining the failure
    fn elle_to_c(&self, value: &Value, ctype: &CType) -> Result<CValue, String>;

    /// Convert a C value back to Elle representation.
    ///
    /// # Arguments
    /// * `cval` - The C value to unmarshal
    /// * `ctype` - The C type information
    ///
    /// # Returns
    /// * `Ok(Value)` - Successfully unmarshaled value
    /// * `Err(String)` - Error message explaining the failure
    fn c_to_elle(&self, cval: &CValue, ctype: &CType) -> Result<Value, String>;

    /// Check if this handler can handle the given type.
    ///
    /// This allows handlers to be more flexible in what they can process.
    fn can_handle(&self, ctype: &CType) -> bool;

    /// Get the priority of this handler (higher = first to try).
    ///
    /// Default is 0. Handlers are tried in descending priority order.
    fn priority(&self) -> i32 {
        0
    }
}

/// Handler metadata for debugging and inspection.
#[derive(Debug, Clone)]
pub struct HandlerMetadata {
    pub type_id: TypeId,
    pub priority: i32,
    pub created_at: std::time::SystemTime,
}

/// Type alias for handler storage: type ID -> (handler, metadata)
type HandlerStorage = HashMap<TypeId, (Arc<dyn TypeHandler>, HandlerMetadata)>;

/// Registry for managing custom type handlers.
///
/// This thread-safe registry manages custom type handlers with support for
/// priority-based dispatch and handler composition.
pub struct HandlerRegistry {
    handlers: Arc<Mutex<HandlerStorage>>,
}

impl HandlerRegistry {
    /// Create a new empty handler registry.
    pub fn new() -> Self {
        HandlerRegistry {
            handlers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a custom type handler.
    ///
    /// # Arguments
    /// * `type_id` - The type identifier
    /// * `handler` - The handler implementation
    ///
    /// # Returns
    /// * `Ok(())` - Handler registered successfully
    /// * `Err(String)` - Registration failed
    pub fn register(&self, type_id: TypeId, handler: Arc<dyn TypeHandler>) -> Result<(), String> {
        let metadata = HandlerMetadata {
            type_id: type_id.clone(),
            priority: handler.priority(),
            created_at: std::time::SystemTime::now(),
        };

        let mut handlers = self
            .handlers
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        handlers.insert(type_id.clone(), (handler.clone(), metadata));
        Ok(())
    }

    /// Unregister a custom type handler.
    ///
    /// # Arguments
    /// * `type_id` - The type identifier to remove
    ///
    /// # Returns
    /// * `Ok(())` - Handler removed successfully
    /// * `Err(String)` - Handler not found or lock error
    pub fn unregister(&self, type_id: &TypeId) -> Result<(), String> {
        let mut handlers = self
            .handlers
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        handlers
            .remove(type_id)
            .ok_or_else(|| format!("Handler not found: {}", type_id.name()))?;
        Ok(())
    }

    /// Get a handler for a specific type ID.
    ///
    /// # Arguments
    /// * `type_id` - The type identifier to look up
    ///
    /// # Returns
    /// * `Some(handler)` - Handler found
    /// * `None` - No handler registered for this type
    pub fn get(&self, type_id: &TypeId) -> Option<Arc<dyn TypeHandler>> {
        let handlers = self.handlers.lock().ok()?;
        handlers.get(type_id).map(|(handler, _)| handler.clone())
    }

    /// Get handler metadata.
    pub fn get_metadata(&self, type_id: &TypeId) -> Option<HandlerMetadata> {
        let handlers = self.handlers.lock().ok()?;
        handlers.get(type_id).map(|(_, metadata)| metadata.clone())
    }

    /// Find the best handler for a given C type.
    ///
    /// Searches through registered handlers and returns the one with the
    /// highest priority that can handle the type.
    ///
    /// # Arguments
    /// * `ctype` - The C type to find a handler for
    ///
    /// # Returns
    /// * `Some((type_id, handler))` - Best matching handler
    /// * `None` - No handler found
    pub fn find_handler(&self, ctype: &CType) -> Option<(TypeId, Arc<dyn TypeHandler>)> {
        let handlers = self.handlers.lock().ok()?;

        // Collect candidates and sort by priority (descending)
        let mut candidates: Vec<_> = handlers
            .iter()
            .filter(|(_, (handler, _))| handler.can_handle(ctype))
            .map(|(type_id, (handler, metadata))| {
                (type_id.clone(), handler.clone(), metadata.priority)
            })
            .collect();

        candidates.sort_by(|a, b| b.2.cmp(&a.2)); // Sort descending by priority

        candidates
            .into_iter()
            .next()
            .map(|(type_id, handler, _)| (type_id, handler))
    }

    /// List all registered handlers.
    pub fn list_handlers(&self) -> Result<Vec<(TypeId, HandlerMetadata)>, String> {
        let handlers = self
            .handlers
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        Ok(handlers
            .iter()
            .map(|(type_id, (_, metadata))| (type_id.clone(), metadata.clone()))
            .collect())
    }

    /// Clear all handlers.
    pub fn clear(&self) -> Result<(), String> {
        let mut handlers = self
            .handlers
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        handlers.clear();
        Ok(())
    }

    /// Check if a handler is registered for the given type ID.
    pub fn has_handler(&self, type_id: &TypeId) -> Result<bool, String> {
        let handlers = self
            .handlers
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        Ok(handlers.contains_key(type_id))
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for HandlerRegistry {
    fn clone(&self) -> Self {
        HandlerRegistry {
            handlers: Arc::clone(&self.handlers),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SimpleHandler {
        priority: i32,
    }

    impl TypeHandler for SimpleHandler {
        fn elle_to_c(&self, value: &Value, _ctype: &CType) -> Result<CValue, String> {
            match value {
                Value::Int(n) => Ok(CValue::Int(*n)),
                _ => Err("Can only handle integers".to_string()),
            }
        }

        fn c_to_elle(&self, cval: &CValue, _ctype: &CType) -> Result<Value, String> {
            match cval {
                CValue::Int(n) => Ok(Value::Int(*n)),
                _ => Err("Can only handle integers".to_string()),
            }
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
        let type_id = TypeId::new("MyType");
        let handler = Arc::new(SimpleHandler { priority: 0 });

        assert!(registry.register(type_id.clone(), handler).is_ok());
        assert!(registry.has_handler(&type_id).unwrap());
    }

    #[test]
    fn test_handler_unregistration() {
        let registry = HandlerRegistry::new();
        let type_id = TypeId::new("MyType");
        let handler = Arc::new(SimpleHandler { priority: 0 });

        registry.register(type_id.clone(), handler).unwrap();
        assert!(registry.has_handler(&type_id).unwrap());

        assert!(registry.unregister(&type_id).is_ok());
        assert!(!registry.has_handler(&type_id).unwrap());
    }

    #[test]
    fn test_handler_priority() {
        let registry = HandlerRegistry::new();
        let type_id1 = TypeId::new("Type1");
        let type_id2 = TypeId::new("Type2");

        let handler1 = Arc::new(SimpleHandler { priority: 10 });
        let handler2 = Arc::new(SimpleHandler { priority: 20 });

        registry.register(type_id1, handler1).unwrap();
        registry.register(type_id2, handler2).unwrap();

        let (found_id, _) = registry.find_handler(&CType::Int).unwrap();
        assert_eq!(found_id.name(), "Type2");
    }

    #[test]
    fn test_handler_metadata() {
        let registry = HandlerRegistry::new();
        let type_id = TypeId::new("MyType");
        let handler = Arc::new(SimpleHandler { priority: 5 });

        registry.register(type_id.clone(), handler).unwrap();

        let metadata = registry.get_metadata(&type_id).unwrap();
        assert_eq!(metadata.type_id.name(), "MyType");
        assert_eq!(metadata.priority, 5);
    }

    #[test]
    fn test_list_handlers() {
        let registry = HandlerRegistry::new();
        let type_id1 = TypeId::new("Type1");
        let type_id2 = TypeId::new("Type2");

        let handler1 = Arc::new(SimpleHandler { priority: 0 });
        let handler2 = Arc::new(SimpleHandler { priority: 0 });

        registry.register(type_id1, handler1).unwrap();
        registry.register(type_id2, handler2).unwrap();

        let handlers = registry.list_handlers().unwrap();
        assert_eq!(handlers.len(), 2);
    }

    #[test]
    fn test_clear_handlers() {
        let registry = HandlerRegistry::new();
        let type_id = TypeId::new("MyType");
        let handler = Arc::new(SimpleHandler { priority: 0 });

        registry.register(type_id.clone(), handler).unwrap();
        assert!(registry.has_handler(&type_id).unwrap());

        assert!(registry.clear().is_ok());
        assert!(!registry.has_handler(&type_id).unwrap());
    }
}
