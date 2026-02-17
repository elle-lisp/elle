//! Condition objects for the exception system
//!
//! Conditions are structured exception objects that carry:
//! - An exception ID (for fast matching)
//! - A mandatory message (human-readable description)
//! - Field values (structured data about the condition)
//! - Optional backtrace (for debugging)
//! - Optional source location (for error reporting)

use crate::reader::SourceLoc;
use crate::value::Value;
use std::collections::HashMap;
use std::fmt;

/// Exception hierarchy: (child_id, parent_id)
const HIERARCHY: &[(u32, u32)] = &[
    (0, 1), // generic -> condition (legacy compatibility)
    (2, 1), // error -> condition
    (3, 2), // type-error -> error
    (4, 2), // division-by-zero -> error
    (5, 2), // undefined-variable -> error
    (6, 2), // arity-error -> error
    (7, 1), // warning -> condition
    (8, 7), // style-warning -> warning
];

/// Get the parent exception ID for a given exception ID
pub fn exception_parent(exception_id: u32) -> Option<u32> {
    HIERARCHY
        .iter()
        .find(|(child, _)| *child == exception_id)
        .map(|(_, parent)| *parent)
}

/// Check if child exception ID is a subclass of parent exception ID
pub fn is_exception_subclass(child_id: u32, parent_id: u32) -> bool {
    if child_id == parent_id {
        return true;
    }
    let mut current = child_id;
    while let Some(parent) = exception_parent(current) {
        if parent == parent_id {
            return true;
        }
        current = parent;
    }
    false
}

/// Get human-readable name for an exception ID
pub fn exception_name(exception_id: u32) -> &'static str {
    match exception_id {
        0 => "Exception",
        1 => "condition",
        2 => "error",
        3 => "type-error",
        4 => "division-by-zero",
        5 => "undefined-variable",
        6 => "arity-error",
        7 => "warning",
        8 => "style-warning",
        _ => "unknown-exception",
    }
}

/// A condition object representing an exceptional situation
#[derive(Debug, Clone)]
pub struct Condition {
    /// Exception type ID (compiled at compile-time)
    pub exception_id: u32,
    /// Human-readable message (mandatory)
    pub message: String,
    /// Field values (field_id -> value mapping)
    pub fields: HashMap<u32, Value>,
    /// Optional backtrace for debugging
    pub backtrace: Option<String>,
    /// Optional source location for error reporting
    pub location: Option<SourceLoc>,
}

impl Condition {
    /// Reserved exception ID for generic exceptions (legacy Exception type)
    pub const GENERIC_EXCEPTION_ID: u32 = 0;

    /// Reserved field ID for exception data
    pub const FIELD_DATA: u32 = 1;

    /// Create a new condition with given exception ID and message (crate-private)
    pub(crate) fn new(exception_id: u32, message: impl Into<String>) -> Self {
        Condition {
            exception_id,
            message: message.into(),
            fields: HashMap::new(),
            backtrace: None,
            location: None,
        }
    }

    // Named constructors

    /// Create a generic exception (ID 0)
    pub fn generic(msg: impl Into<String>) -> Self {
        Self::new(0, msg)
    }

    /// Create a base condition (ID 1)
    pub fn base_condition(msg: impl Into<String>) -> Self {
        Self::new(1, msg)
    }

    /// Create an error (ID 2)
    pub fn error(msg: impl Into<String>) -> Self {
        Self::new(2, msg)
    }

    /// Create a type error (ID 3)
    pub fn type_error(msg: impl Into<String>) -> Self {
        Self::new(3, msg)
    }

    /// Create a division by zero error (ID 4)
    pub fn division_by_zero(msg: impl Into<String>) -> Self {
        Self::new(4, msg)
    }

    /// Create an undefined variable error (ID 5)
    pub fn undefined_variable(msg: impl Into<String>) -> Self {
        Self::new(5, msg)
    }

    /// Create an arity error (ID 6)
    pub fn arity_error(msg: impl Into<String>) -> Self {
        Self::new(6, msg)
    }

    /// Create a warning (ID 7)
    pub fn warning(msg: impl Into<String>) -> Self {
        Self::new(7, msg)
    }

    /// Create a style warning (ID 8)
    pub fn style_warning(msg: impl Into<String>) -> Self {
        Self::new(8, msg)
    }

    /// Create a generic exception with message and data
    pub fn generic_with_data(message: impl Into<String>, data: Value) -> Self {
        let mut cond = Self::generic(message);
        cond.fields.insert(Self::FIELD_DATA, data);
        cond
    }

    // Builder methods

    /// Add a field value (builder pattern)
    pub fn with_field(mut self, field_id: u32, value: Value) -> Self {
        self.fields.insert(field_id, value);
        self
    }

    /// Set backtrace information
    pub fn with_backtrace(mut self, backtrace: String) -> Self {
        self.backtrace = Some(backtrace);
        self
    }

    /// Set source location information
    pub fn with_location(mut self, loc: SourceLoc) -> Self {
        self.location = Some(loc);
        self
    }

    // Accessors

    /// Set a field value
    pub fn set_field(&mut self, field_id: u32, value: Value) {
        self.fields.insert(field_id, value);
    }

    /// Get a field value
    pub fn get_field(&self, field_id: u32) -> Option<&Value> {
        self.fields.get(&field_id)
    }

    /// Get the message (always present)
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Get data for generic exceptions (field 1)
    pub fn data(&self) -> Option<&Value> {
        self.get_field(Self::FIELD_DATA)
    }

    /// Check if this is a generic exception
    pub fn is_generic(&self) -> bool {
        self.exception_id == Self::GENERIC_EXCEPTION_ID
    }

    /// Check if this condition is of a specific type (including inheritance)
    pub fn is_instance_of(&self, exception_id: u32) -> bool {
        is_exception_subclass(self.exception_id, exception_id)
    }
}

impl PartialEq for Condition {
    fn eq(&self, other: &Self) -> bool {
        self.exception_id == other.exception_id
            && self.message == other.message
            && self.fields == other.fields
            && self.location == other.location
    }
}

impl fmt::Display for Condition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = exception_name(self.exception_id);
        write!(f, "{}: {}", name, self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_named_constructors_produce_correct_ids() {
        assert_eq!(Condition::generic("msg").exception_id, 0);
        assert_eq!(Condition::base_condition("msg").exception_id, 1);
        assert_eq!(Condition::error("msg").exception_id, 2);
        assert_eq!(Condition::type_error("msg").exception_id, 3);
        assert_eq!(Condition::division_by_zero("msg").exception_id, 4);
        assert_eq!(Condition::undefined_variable("msg").exception_id, 5);
        assert_eq!(Condition::arity_error("msg").exception_id, 6);
        assert_eq!(Condition::warning("msg").exception_id, 7);
        assert_eq!(Condition::style_warning("msg").exception_id, 8);
    }

    #[test]
    fn test_named_constructors_store_message() {
        let cond = Condition::type_error("expected pair, got integer");
        assert_eq!(cond.message(), "expected pair, got integer");
    }

    #[test]
    fn test_is_instance_of_respects_hierarchy() {
        // type-error is-instance-of error
        let te = Condition::type_error("msg");
        assert!(te.is_instance_of(3)); // itself
        assert!(te.is_instance_of(2)); // error
        assert!(te.is_instance_of(1)); // condition
        assert!(!te.is_instance_of(7)); // not warning

        // error is-instance-of condition
        let err = Condition::error("msg");
        assert!(err.is_instance_of(2)); // itself
        assert!(err.is_instance_of(1)); // condition
        assert!(!err.is_instance_of(3)); // not type-error (child)

        // style-warning is-instance-of warning
        let sw = Condition::style_warning("msg");
        assert!(sw.is_instance_of(8)); // itself
        assert!(sw.is_instance_of(7)); // warning
        assert!(sw.is_instance_of(1)); // condition
        assert!(!sw.is_instance_of(2)); // not error
    }

    #[test]
    fn test_with_field_builder() {
        let cond = Condition::division_by_zero("division by zero")
            .with_field(0, Value::int(42))
            .with_field(1, Value::int(0));

        assert_eq!(cond.get_field(0), Some(&Value::int(42)));
        assert_eq!(cond.get_field(1), Some(&Value::int(0)));
    }

    #[test]
    fn test_display_formatting() {
        let generic = Condition::generic("test error");
        assert_eq!(generic.to_string(), "Exception: test error");

        let te = Condition::type_error("car: expected pair, got integer");
        assert_eq!(
            te.to_string(),
            "type-error: car: expected pair, got integer"
        );

        let dbz = Condition::division_by_zero("cannot divide by zero");
        assert_eq!(dbz.to_string(), "division-by-zero: cannot divide by zero");
    }

    #[test]
    fn test_message_returns_str() {
        let cond = Condition::error("hello world");
        let msg: &str = cond.message();
        assert_eq!(msg, "hello world");
    }

    #[test]
    fn test_generic_with_data() {
        let data = Value::int(42);
        let cond = Condition::generic_with_data("error with data", data);
        assert_eq!(cond.exception_id, 0);
        assert_eq!(cond.message(), "error with data");
        assert_eq!(cond.data(), Some(&Value::int(42)));
    }

    #[test]
    fn test_exception_parent() {
        assert_eq!(exception_parent(0), Some(1)); // generic -> condition
        assert_eq!(exception_parent(2), Some(1)); // error -> condition
        assert_eq!(exception_parent(3), Some(2)); // type-error -> error
        assert_eq!(exception_parent(4), Some(2)); // division-by-zero -> error
        assert_eq!(exception_parent(5), Some(2)); // undefined-variable -> error
        assert_eq!(exception_parent(6), Some(2)); // arity-error -> error
        assert_eq!(exception_parent(7), Some(1)); // warning -> condition
        assert_eq!(exception_parent(8), Some(7)); // style-warning -> warning
        assert_eq!(exception_parent(1), None); // condition has no parent
        assert_eq!(exception_parent(99), None); // unknown has no parent
    }

    #[test]
    fn test_is_exception_subclass() {
        // Same ID
        assert!(is_exception_subclass(3, 3));

        // Direct parent
        assert!(is_exception_subclass(3, 2)); // type-error -> error
        assert!(is_exception_subclass(2, 1)); // error -> condition

        // Transitive parent
        assert!(is_exception_subclass(3, 1)); // type-error -> condition

        // Not related
        assert!(!is_exception_subclass(3, 7)); // type-error not subclass of warning
        assert!(!is_exception_subclass(2, 3)); // error not subclass of type-error
    }

    #[test]
    fn test_exception_names() {
        assert_eq!(exception_name(0), "Exception");
        assert_eq!(exception_name(1), "condition");
        assert_eq!(exception_name(2), "error");
        assert_eq!(exception_name(3), "type-error");
        assert_eq!(exception_name(4), "division-by-zero");
        assert_eq!(exception_name(5), "undefined-variable");
        assert_eq!(exception_name(6), "arity-error");
        assert_eq!(exception_name(7), "warning");
        assert_eq!(exception_name(8), "style-warning");
        assert_eq!(exception_name(99), "unknown-exception");
    }

    #[test]
    fn test_condition_equality() {
        let cond1 = Condition::error("test").with_field(0, Value::int(42));
        let cond2 = Condition::error("test").with_field(0, Value::int(42));
        let cond3 = Condition::error("different");

        assert_eq!(cond1, cond2);
        assert_ne!(cond1, cond3);
    }

    #[test]
    fn test_is_generic() {
        assert!(Condition::generic("test").is_generic());
        assert!(!Condition::error("test").is_generic());
    }

    #[test]
    fn test_constants() {
        assert_eq!(Condition::GENERIC_EXCEPTION_ID, 0);
        assert_eq!(Condition::FIELD_DATA, 1);
    }
}
