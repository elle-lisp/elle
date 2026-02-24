//! Core value types for the Elle runtime
//!
//! This module contains fundamental types used throughout the value system:
//! - `SymbolId` - Interned symbol identifier
//! - `Arity` - Function arity specification
//! - `TableKey` - Keys for tables and structs
//! - `NativeFn` - Unified primitive function type

use crate::error::{LError, LResult};
use crate::value::Value;
use std::fmt;

/// Symbol ID for interned symbols.
///
/// Symbols are interned for fast comparison (O(1) via ID comparison
/// instead of O(n) string comparison).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(pub u32);

impl fmt::Display for SymbolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Symbol({})", self.0)
    }
}

/// Function arity specification.
///
/// Specifies how many arguments a function accepts.
///
/// # Examples
///
/// ```
/// use elle::value::Arity;
/// assert!(Arity::Exact(2).matches(2));
/// assert!(!Arity::Exact(2).matches(1));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    /// Exact number of arguments required
    Exact(usize),
    /// At least this many arguments
    AtLeast(usize),
    /// Between min and max arguments (inclusive)
    Range(usize, usize),
}

impl Arity {
    pub fn matches(&self, n: usize) -> bool {
        match self {
            Arity::Exact(expected) => n == *expected,
            Arity::AtLeast(min) => n >= *min,
            Arity::Range(min, max) => n >= *min && n <= *max,
        }
    }

    /// Number of fixed parameter slots this arity requires.
    /// For `Exact(n)` → n, for `AtLeast(n)` → n, for `Range(min, _)` → min.
    pub fn fixed_params(&self) -> usize {
        match self {
            Arity::Exact(n) | Arity::AtLeast(n) | Arity::Range(n, _) => *n,
        }
    }
}

impl fmt::Display for Arity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Arity::Exact(n) => write!(f, "{}", n),
            Arity::AtLeast(n) => write!(f, "{}+", n),
            Arity::Range(min, max) => write!(f, "{}-{}", min, max),
        }
    }
}

/// Wrapper for table/struct keys - allows specific Value types to be keys
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TableKey {
    Nil,
    Bool(bool),
    Int(i64),
    Symbol(SymbolId),
    String(String),
}

impl TableKey {
    /// Convert a Value to a TableKey if possible
    pub fn from_value(val: &Value) -> LResult<TableKey> {
        if val.is_nil() {
            Ok(TableKey::Nil)
        } else if let Some(b) = val.as_bool() {
            Ok(TableKey::Bool(b))
        } else if let Some(i) = val.as_int() {
            Ok(TableKey::Int(i))
        } else if let Some(id) = val.as_symbol() {
            Ok(TableKey::Symbol(SymbolId(id)))
        } else if let Some(s) = val.as_string() {
            Ok(TableKey::String(s.to_string()))
        } else {
            Err(LError::type_mismatch("table key", val.type_name()))
        }
    }
}

impl std::hash::Hash for TableKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            TableKey::Nil => {}
            TableKey::Bool(b) => b.hash(state),
            TableKey::Int(i) => i.hash(state),
            TableKey::Symbol(id) => id.hash(state),
            TableKey::String(s) => s.hash(state),
        }
    }
}

/// Unified primitive function type.
///
/// All primitives return (signal_bits, value):
/// - (SIG_OK, value) → push value onto stack
/// - (SIG_ERROR, condition_value) → set fiber.current_exception
/// - (SIG_YIELD, value) → store in fiber.signal, suspend
/// - (SIG_RESUME, fiber_value) → VM does fiber swap
///
/// No primitive has VM access. Operations that formerly needed the VM
/// now emit signals that the VM dispatch loop handles.
pub type NativeFn = fn(&[Value]) -> (crate::value::fiber::SignalBits, Value);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arity_matches() {
        assert!(Arity::Exact(2).matches(2));
        assert!(!Arity::Exact(2).matches(1));
        assert!(!Arity::Exact(2).matches(3));

        assert!(Arity::AtLeast(2).matches(2));
        assert!(Arity::AtLeast(2).matches(3));
        assert!(!Arity::AtLeast(2).matches(1));

        assert!(Arity::Range(1, 3).matches(1));
        assert!(Arity::Range(1, 3).matches(2));
        assert!(Arity::Range(1, 3).matches(3));
        assert!(!Arity::Range(1, 3).matches(0));
        assert!(!Arity::Range(1, 3).matches(4));
    }

    #[test]
    fn test_arity_display() {
        assert_eq!(format!("{}", Arity::Exact(2)), "2");
        assert_eq!(format!("{}", Arity::AtLeast(1)), "1+");
        assert_eq!(format!("{}", Arity::Range(1, 3)), "1-3");
    }

    #[test]
    fn test_symbol_id_display() {
        assert_eq!(format!("{}", SymbolId(42)), "Symbol(42)");
    }
}
