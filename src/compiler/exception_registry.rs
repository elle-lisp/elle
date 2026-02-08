//! Exception registry for compile-time exception type registration
//!
//! This module manages the mapping of exception names to numeric IDs,
//! field definitions, and inheritance relationships. All exception types
//! must be explicitly declared via `define-exception` before use.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub name: String,
    pub name_symbol_id: u32, // For field matching
}

#[derive(Debug, Clone)]
pub struct ExceptionInfo {
    pub id: u32,
    pub name: String,
    pub fields: Vec<FieldInfo>,
    pub parent: Option<String>, // For inheritance hierarchy
}

#[derive(Debug, Clone)]
pub struct ExceptionRegistry {
    exceptions: HashMap<String, ExceptionInfo>,
    next_id: u32,
}

impl ExceptionRegistry {
    pub fn new() -> Self {
        let mut registry = ExceptionRegistry {
            exceptions: HashMap::new(),
            next_id: 1, // 0 is reserved
        };

        // Register built-in exception hierarchy
        registry.register_builtin_exceptions();
        registry
    }

    fn register_builtin_exceptions(&mut self) {
        // Base condition type
        self.register_exception("condition", vec![], None);

        // Error hierarchy
        self.register_exception("error", vec![], Some("condition"));

        self.register_exception(
            "type-error",
            vec![
                FieldInfo {
                    name: "expected".to_string(),
                    name_symbol_id: 0, // Will be set by compiler
                },
                FieldInfo {
                    name: "actual".to_string(),
                    name_symbol_id: 0,
                },
            ],
            Some("error"),
        );

        self.register_exception(
            "division-by-zero",
            vec![
                FieldInfo {
                    name: "dividend".to_string(),
                    name_symbol_id: 0,
                },
                FieldInfo {
                    name: "divisor".to_string(),
                    name_symbol_id: 0,
                },
            ],
            Some("error"),
        );

        self.register_exception(
            "undefined-variable",
            vec![FieldInfo {
                name: "variable".to_string(),
                name_symbol_id: 0,
            }],
            Some("error"),
        );

        self.register_exception(
            "arity-error",
            vec![
                FieldInfo {
                    name: "expected".to_string(),
                    name_symbol_id: 0,
                },
                FieldInfo {
                    name: "actual".to_string(),
                    name_symbol_id: 0,
                },
            ],
            Some("error"),
        );

        // Warning hierarchy
        self.register_exception("warning", vec![], Some("condition"));

        self.register_exception("style-warning", vec![], Some("warning"));
    }

    pub fn register_exception(
        &mut self,
        name: &str,
        fields: Vec<FieldInfo>,
        parent: Option<&str>,
    ) -> u32 {
        if self.exceptions.contains_key(name) {
            panic!("Exception '{}' already registered", name);
        }

        let id = self.next_id;
        self.next_id += 1;

        let info = ExceptionInfo {
            id,
            name: name.to_string(),
            fields,
            parent: parent.map(|p| p.to_string()),
        };

        self.exceptions.insert(name.to_string(), info);
        id
    }

    pub fn get_exception(&self, name: &str) -> Option<&ExceptionInfo> {
        self.exceptions.get(name)
    }

    pub fn get_exception_by_id(&self, id: u32) -> Option<&ExceptionInfo> {
        self.exceptions.values().find(|exc| exc.id == id)
    }

    pub fn exception_exists(&self, name: &str) -> bool {
        self.exceptions.contains_key(name)
    }

    pub fn is_subclass(&self, child: &str, parent: &str) -> bool {
        let mut current = child.to_string();
        while let Some(exc_info) = self.get_exception(&current) {
            if current == parent {
                return true;
            }
            if let Some(parent_name) = &exc_info.parent {
                current = parent_name.clone();
            } else {
                break;
            }
        }
        false
    }

    pub fn all_exceptions(&self) -> Vec<&ExceptionInfo> {
        let mut excs: Vec<_> = self.exceptions.values().collect();
        excs.sort_by_key(|e| e.id);
        excs
    }
}

impl Default for ExceptionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = ExceptionRegistry::new();
        assert!(registry.exception_exists("error"));
        assert!(registry.exception_exists("type-error"));
        assert!(registry.exception_exists("division-by-zero"));
    }

    #[test]
    fn test_exception_registration() {
        let mut registry = ExceptionRegistry::new();
        let id = registry.register_exception("my-error", vec![], Some("error"));
        assert!(registry.exception_exists("my-error"));
        assert_eq!(registry.get_exception("my-error").unwrap().id, id);
    }

    #[test]
    fn test_inheritance() {
        let registry = ExceptionRegistry::new();
        assert!(registry.is_subclass("type-error", "error"));
        assert!(registry.is_subclass("type-error", "condition"));
        assert!(!registry.is_subclass("error", "type-error"));
    }

    #[test]
    #[should_panic]
    fn test_duplicate_registration() {
        let mut registry = ExceptionRegistry::new();
        registry.register_exception("error", vec![], None);
    }
}
