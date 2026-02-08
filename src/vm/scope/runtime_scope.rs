use crate::compiler::scope::ScopeType;
use crate::value::Value;
use std::collections::HashMap;

/// Represents a single scope level at runtime
#[derive(Debug, Clone)]
pub struct RuntimeScope {
    /// Variables defined at this scope level (symbol_id -> Value)
    pub variables: HashMap<u32, Value>,
    /// Type of this scope
    pub scope_type: ScopeType,
}

impl RuntimeScope {
    /// Create a new runtime scope
    pub fn new(scope_type: ScopeType) -> Self {
        RuntimeScope {
            variables: HashMap::new(),
            scope_type,
        }
    }

    /// Define a variable in this scope
    pub fn define(&mut self, sym_id: u32, value: Value) {
        self.variables.insert(sym_id, value);
    }

    /// Get a variable from this scope
    pub fn get(&self, sym_id: u32) -> Option<&Value> {
        self.variables.get(&sym_id)
    }

    /// Get a mutable reference to a variable
    pub fn get_mut(&mut self, sym_id: u32) -> Option<&mut Value> {
        self.variables.get_mut(&sym_id)
    }

    /// Set a variable in this scope (returns old value if present)
    pub fn set(&mut self, sym_id: u32, value: Value) -> Option<Value> {
        self.variables.insert(sym_id, value)
    }

    /// Check if a variable is defined in this scope
    pub fn contains(&self, sym_id: u32) -> bool {
        self.variables.contains_key(&sym_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_scope_creation() {
        let scope = RuntimeScope::new(ScopeType::Global);
        assert_eq!(scope.scope_type, ScopeType::Global);
        assert!(scope.variables.is_empty());
    }

    #[test]
    fn test_runtime_scope_define_and_get() {
        let mut scope = RuntimeScope::new(ScopeType::Block);
        let sym_id = 42;
        let value = Value::Int(123);

        scope.define(sym_id, value.clone());
        assert_eq!(scope.get(sym_id), Some(&value));
        assert!(scope.contains(sym_id));
    }
}
