//! Core value types for the Elle runtime
//!
//! This module contains fundamental types used throughout the value system:
//! - `SymbolId` - Interned symbol identifier
//! - `Arity` - Function arity specification
//! - `TableKey` - Keys for tables and structs
//! - `NativeFn` - Unified primitive function type

use crate::value::heap::HeapTag;
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
    /// Compute the arity for a lambda with the given parameter structure.
    /// - `has_rest`: whether the function has a rest/keys/named collector
    /// - `num_required`: number of required parameters (before &opt)
    /// - `num_params`: total number of parameter slots (required + optional + rest if present)
    pub fn for_lambda(has_rest: bool, num_required: usize, num_params: usize) -> Self {
        if has_rest {
            Arity::AtLeast(num_required)
        } else if num_required < num_params {
            Arity::Range(num_required, num_params)
        } else {
            Arity::Exact(num_params)
        }
    }

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
#[derive(Debug, Clone)]
pub enum TableKey {
    Nil,
    Bool(bool),
    Int(i64),
    Symbol(SymbolId),
    String(String),
    Keyword(String),
    /// Identity-compared key for reference types (fiber, closure, external).
    ///
    /// **Invariant**: Only constructed via `from_value()`. The stored `Value`
    /// must be a type where `eq?` uses pointer identity (currently: fiber,
    /// closure, external). Storing a value-compared type here would silently
    /// use bit-pattern comparison instead of value comparison.
    ///
    /// Hash/Eq/Ord compare by `Value.0` (the raw NaN-boxed bits), which
    /// encodes the heap pointer. This gives the same identity semantics as
    /// `eq?` for these types.
    Identity(Value),
}

impl TableKey {
    /// Convert a Value to a TableKey if possible.
    ///
    /// Returns `None` if the value cannot be used as a key.
    /// Callers produce their own error messages from the `None` case.
    pub fn from_value(val: &Value) -> Option<TableKey> {
        if val.is_nil() {
            Some(TableKey::Nil)
        } else if let Some(b) = val.as_bool() {
            Some(TableKey::Bool(b))
        } else if let Some(i) = val.as_int() {
            Some(TableKey::Int(i))
        } else if let Some(id) = val.as_symbol() {
            Some(TableKey::Symbol(SymbolId(id)))
        } else if let Some(name) = val.as_keyword_name() {
            Some(TableKey::Keyword(name.to_string()))
        } else if let Some(s) = val.with_string(|s| s.to_string()) {
            Some(TableKey::String(s))
        } else if val.is_fiber() || val.is_closure() || val.heap_tag() == Some(HeapTag::External) {
            Some(TableKey::Identity(*val))
        } else {
            None
        }
    }

    /// Convert a TableKey back to a Value.
    ///
    /// This is the inverse of `from_value()`.
    pub fn to_value(&self) -> Value {
        match self {
            TableKey::Nil => Value::NIL,
            TableKey::Bool(b) => Value::bool(*b),
            TableKey::Int(i) => Value::int(*i),
            TableKey::Symbol(sid) => Value::symbol(sid.0),
            TableKey::String(s) => Value::string(s.as_str()),
            TableKey::Keyword(s) => Value::keyword(s.as_str()),
            TableKey::Identity(v) => *v,
        }
    }

    /// Whether this key can be safely sent across thread boundaries.
    ///
    /// Identity keys contain heap pointers (`Rc`) that are not thread-safe.
    /// Value-based keys (nil, bool, int, symbol, string, keyword) are always
    /// sendable.
    pub fn is_sendable(&self) -> bool {
        !matches!(self, TableKey::Identity(_))
    }

    fn discriminant_index(&self) -> u8 {
        match self {
            TableKey::Nil => 0,
            TableKey::Bool(_) => 1,
            TableKey::Int(_) => 2,
            TableKey::Symbol(_) => 3,
            TableKey::String(_) => 4,
            TableKey::Keyword(_) => 5,
            TableKey::Identity(_) => 6,
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
            TableKey::Keyword(s) => s.hash(state),
            TableKey::Identity(v) => v.0.hash(state),
        }
    }
}

impl PartialEq for TableKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (TableKey::Nil, TableKey::Nil) => true,
            (TableKey::Bool(a), TableKey::Bool(b)) => a == b,
            (TableKey::Int(a), TableKey::Int(b)) => a == b,
            (TableKey::Symbol(a), TableKey::Symbol(b)) => a == b,
            (TableKey::String(a), TableKey::String(b)) => a == b,
            (TableKey::Keyword(a), TableKey::Keyword(b)) => a == b,
            (TableKey::Identity(a), TableKey::Identity(b)) => a.0 == b.0,
            _ => false,
        }
    }
}

impl Eq for TableKey {}

impl PartialOrd for TableKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TableKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Variant ordering follows enum declaration order (same as derive).
        // Discriminant index: Nil=0, Bool=1, Int=2, Symbol=3, String=4, Keyword=5, Identity=6
        let self_disc = self.discriminant_index();
        let other_disc = other.discriminant_index();
        match self_disc.cmp(&other_disc) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        match (self, other) {
            (TableKey::Nil, TableKey::Nil) => std::cmp::Ordering::Equal,
            (TableKey::Bool(a), TableKey::Bool(b)) => a.cmp(b),
            (TableKey::Int(a), TableKey::Int(b)) => a.cmp(b),
            (TableKey::Symbol(a), TableKey::Symbol(b)) => a.cmp(b),
            (TableKey::String(a), TableKey::String(b)) => a.cmp(b),
            (TableKey::Keyword(a), TableKey::Keyword(b)) => a.cmp(b),
            (TableKey::Identity(a), TableKey::Identity(b)) => a.0.cmp(&b.0),
            _ => unreachable!("discriminant match already handled"),
        }
    }
}

impl fmt::Display for TableKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TableKey::Nil => write!(f, "nil"),
            TableKey::Bool(b) => write!(f, "{}", b),
            TableKey::Int(i) => write!(f, "{}", i),
            TableKey::Symbol(id) => write!(f, "{:?}", id),
            TableKey::String(s) => write!(f, "\"{}\"", s),
            TableKey::Keyword(s) => write!(f, ":{}", s),
            TableKey::Identity(v) => write!(f, "{}", v),
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
