// audited: 2026-09-08
//! The value system's small standing types: symbol identity, arity, and the
//! signature every primitive has.
//!
//! docs/impl/symbol.md
//! docs/impl/values.md
//!
//! Two larger subjects live beside this one and re-export through it: key.rs
//! owns `TableKey` and how keys compare, hash, and print, and sorted.rs owns
//! lookup over an immutable struct's sorted entries.

mod key;
mod sorted;

use std::fmt;

use crate::value::Value;

pub use key::TableKey;
pub(crate) use key::{fmt_table_key, TableKeyDisplay};
pub use sorted::{
    sorted_struct_contains, sorted_struct_get, sorted_struct_insert, sorted_struct_remove,
};

/// Symbol identity: the name's 64-bit hash.
///
/// The id is a pure function of the name ([`crate::namehash`]), so it is the
/// same in every symbol table, thread, process, and build — comparison is one
/// integer compare, and a symbol value is portable by construction. Ordering
/// follows the hash, which is deterministic but carries no alphabetical
/// meaning; see [docs/impl/symbol.md](../../docs/impl/symbol.md).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct SymbolId(pub u64);

impl SymbolId {
    /// The id of `name`, without recording the name for display. Use
    /// [`SymbolTable::intern`](crate::symbol::SymbolTable::intern) when the
    /// symbol may need to be printed.
    pub const fn of(name: &str) -> Self {
        Self(crate::namehash::name_hash(name))
    }

    /// Sentinel for compiler-generated bindings with no source-level symbol
    /// name (phi temporaries, etc.). Reserved: `SymbolTable::intern` refuses a
    /// name that hashes onto it, so it is never a real symbol.
    pub const SYNTHETIC: Self = Self(u64::MAX);
}

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

/// Primitive function signature.
///
/// All primitives return (signal_bits, value):
/// - (SIG_OK, value) → push value onto stack
/// - (SIG_ERROR, condition_value) → set fiber.current_exception
/// - (SIG_YIELD, value) → store in fiber.signal, suspend
/// - (SIG_RESUME, fiber_value) → VM does fiber swap
///
/// Primitives reach the VM through `ctx.vm()` on their `&mut NativeCtx`.
/// Operations that the primitive cannot perform directly (fiber swaps,
/// resumption) are requested by emitting a signal that the VM dispatch loop
/// handles.
///
/// The leading `&mut NativeCtx` is the allocation capability — the call's
/// own fresh result region plus heap access (docs/impl/region/ctx.md). A
/// primitive cannot allocate without it, and only into its own call's region.
pub type PrimFn = fn(
    &mut crate::primitives::ctx::NativeCtx<'_>,
    &[Value],
) -> (crate::value::fiber::SignalBits, Value);

/// A reference to a static primitive definition. Stored in HeapObject::NativeFn
/// so the VM can access signal metadata at call time for capability enforcement.
pub type NativeFn = &'static crate::primitives::def::PrimitiveDef;

#[cfg(test)]
mod tests;
