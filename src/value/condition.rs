//! Condition objects for the exception system
//!
//! Conditions are structured exception objects that carry:
//! - An exception ID (for fast matching)
//! - Field values (structured data about the condition)
//! - Optional backtrace (for debugging)

use crate::value::Value;
use std::collections::HashMap;
use std::fmt;

/// A condition object representing an exceptional situation
#[derive(Debug, Clone)]
pub struct Condition {
    /// Exception type ID (compiled at compile-time)
    pub exception_id: u32,
    /// Field values (field_id -> value mapping)
    pub fields: HashMap<u32, Value>,
    /// Optional backtrace for debugging
    pub backtrace: Option<String>,
}

impl Condition {
    /// Create a new condition with given exception ID
    pub fn new(exception_id: u32) -> Self {
        Condition {
            exception_id,
            fields: HashMap::new(),
            backtrace: None,
        }
    }

    /// Set a field value
    pub fn set_field(&mut self, field_id: u32, value: Value) {
        self.fields.insert(field_id, value);
    }

    /// Get a field value
    pub fn get_field(&self, field_id: u32) -> Option<&Value> {
        self.fields.get(&field_id)
    }

    /// Set backtrace information
    pub fn with_backtrace(mut self, backtrace: String) -> Self {
        self.backtrace = Some(backtrace);
        self
    }

    /// Check if this condition is of a specific type (including inheritance)
    pub fn is_instance_of(&self, exception_id: u32) -> bool {
        self.exception_id == exception_id
    }
}

impl PartialEq for Condition {
    fn eq(&self, other: &Self) -> bool {
        // Conditions are equal if they have the same ID and field values
        self.exception_id == other.exception_id && self.fields == other.fields
    }
}

impl fmt::Display for Condition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Condition(id={})", self.exception_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_condition_creation() {
        let cond = Condition::new(1);
        assert_eq!(cond.exception_id, 1);
        assert!(cond.fields.is_empty());
    }

    #[test]
    fn test_condition_fields() {
        let mut cond = Condition::new(1);
        cond.set_field(0, Value::Int(42));
        assert_eq!(cond.get_field(0), Some(&Value::Int(42)));
    }

    #[test]
    fn test_condition_equality() {
        let mut cond1 = Condition::new(1);
        cond1.set_field(0, Value::Int(42));

        let mut cond2 = Condition::new(1);
        cond2.set_field(0, Value::Int(42));

        assert_eq!(cond1, cond2);
    }
}
