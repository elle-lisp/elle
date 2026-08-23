//! Core value types for the Elle runtime
//!
//! This module contains fundamental types used throughout the value system:
//! - `SymbolId` - Symbol identity: the name's hash (`docs/impl/symbol.md`)
//! - `Arity` - Function arity specification
//! - `TableKey` - Keys for structs (accepts all immutable non-float types)
//! - `NativeFn` - Unified primitive function type
//!
//! ## TableKey design
//!
//! `TableKey` accepts any immutable, non-float value as a struct key:
//! - **Scalar**: nil, bool, int, symbol, keyword, string, empty list
//! - **Compound immutable**: arrays, cons cells, empty list, bytes, sets, structs
//! - **Identity types**: fiber, closure, external (compared by pointer identity)
//!
//! Mutable types (@array, @struct, @set, @bytes, @string, box) and floats are
//! rejected. Mutable values could change after insertion, breaking hash invariants.
//! Floats are rejected because NaN violates Eq/Hash.
//!
//! Compound immutable keys store the original `Value` directly in the `Heap`
//! variant; `from_value()` recursively validates sub-elements but always stores
//! the original value, not a reconstructed copy.

use crate::primitives::ctx::NativeCtx;
use crate::value::heap::HeapTag;
use crate::value::Value;
use std::fmt;

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

/// Wrapper for table/struct keys - allows specific Value types to be keys
#[derive(Clone)]
pub enum TableKey {
    Nil,
    Bool(bool),
    Int(i64),
    Symbol(SymbolId),
    String(String),
    /// A keyword key is its 64-bit name hash — the keyword value's payload,
    /// exactly as `Symbol` is its `SymbolId`. Identity and order are pure
    /// functions of the hash, so a probe builds a key with no lock and no
    /// allocation, and a sorted struct probes correctly in every instance.
    /// Display resolves the spelling through the per-instance memo
    /// (docs/impl/symbol.md § "The display memo").
    Keyword(u64),
    EmptyList,
    /// Immutable array key. All elements must themselves be valid TableKeys.
    /// Mutable arrays are rejected — mutation after insertion would break
    /// the hash invariant.
    Array(Vec<TableKey>),
    /// Any non-scalar immutable heap value used as a struct key.
    ///
    /// Stores the original `Value` directly. `from_value()` recursively validates
    /// that all sub-elements are themselves valid keys (immutable, non-float) but
    /// always stores the original `*val`, not a reconstructed copy.
    ///
    /// `Hash`/`Eq`/`Ord` delegate to `Value`'s implementations, which give:
    /// - **Identity semantics** for fiber, closure, external (compared by pointer)
    /// - **Structural semantics** for cons, set, struct, bytes, empty list
    Heap(Value),
}

impl TableKey {
    /// Build a keyword key from its spelling. Identity only — the spelling is
    /// not recorded anywhere; display resolves through the per-instance memo.
    pub fn keyword(name: &str) -> TableKey {
        TableKey::Keyword(crate::value::keyword::keyword_hash(name))
    }

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
            Some(TableKey::Symbol(id))
        } else if let Some(hash) = val.keyword_hash() {
            Some(TableKey::Keyword(hash))
        } else if let Some(s) = val.with_string(|s| s.to_string()) {
            Some(TableKey::String(s))
        } else if let Some(arr) = val.as_array() {
            let mut keys = Vec::with_capacity(arr.len());
            for elem in arr {
                keys.push(TableKey::from_value(elem)?);
            }
            Some(TableKey::Array(keys))
        } else if val.is_empty_list() {
            Some(TableKey::EmptyList)
        } else if val.is_pair() {
            let pair = val.as_pair().unwrap();
            Self::from_value(&pair.first)?;
            Self::from_value(&pair.rest)?;
            Some(TableKey::Heap(*val))
        } else if val.is_bytes() {
            Some(TableKey::Heap(*val))
        } else if val.is_set() {
            let set = val.as_set().unwrap();
            for elem in set {
                Self::from_value(elem)?;
            }
            Some(TableKey::Heap(*val))
        } else if val.is_struct() {
            let entries = val.as_struct().unwrap();
            for (_key, value) in entries {
                Self::from_value(value)?;
            }
            Some(TableKey::Heap(*val))
        } else if val.is_fiber() || val.is_closure() || val.heap_tag() == Some(HeapTag::External) {
            Some(TableKey::Heap(*val))
        } else {
            None
        }
    }

    /// Convert a TableKey back to a Value, born in the call's region
    /// (`ctx`). This is the inverse of `from_value()`. String and array keys
    /// allocate (through `ctx`); scalar/keyword/heap keys are immediates or
    /// pass-throughs and need no region.
    pub fn to_value(&self, ctx: &mut NativeCtx) -> Value {
        match self {
            TableKey::Nil => Value::NIL,
            TableKey::Bool(b) => Value::bool(*b),
            TableKey::Int(i) => Value::int(*i),
            TableKey::Symbol(sid) => Value::symbol(*sid),
            TableKey::String(s) => ctx.string(s.as_str()),
            TableKey::Keyword(hash) => Value::keyword_from_hash(*hash),
            TableKey::EmptyList => Value::EMPTY_LIST,
            TableKey::Array(keys) => {
                let items: Vec<Value> = keys.iter().map(|k| k.to_value(ctx)).collect();
                ctx.array(items)
            }
            TableKey::Heap(v) => *v,
        }
    }

    /// Visit every heap `Value` this key holds — the cross-region references a
    /// struct key contributes to its owning struct's region.
    ///
    /// A `Heap` key stores a `Value` pointing into the region the key value was
    /// born in; an `Array` key can nest `Heap` keys among its elements. Scalar
    /// keys (nil/bool/int/symbol/string/keyword/empty-list) carry no region
    /// reference. The region scan (`find_object_cross_refs`) walks these so a
    /// struct increfs and records the edge to each heap key's region at alloc,
    /// balanced by the free-time cascade — the same accounting struct VALUES get.
    pub fn for_each_heap_value(&self, f: &mut impl FnMut(&Value)) {
        match self {
            TableKey::Heap(v) => f(v),
            TableKey::Array(keys) => {
                for k in keys {
                    k.for_each_heap_value(f);
                }
            }
            TableKey::Nil
            | TableKey::Bool(_)
            | TableKey::Int(_)
            | TableKey::Symbol(_)
            | TableKey::String(_)
            | TableKey::Keyword(_)
            | TableKey::EmptyList => {}
        }
    }

    /// Whether this key can be safely sent across thread boundaries.
    ///
    /// Heap keys contain `Rc` data that is not thread-safe.
    /// Value-based keys (nil, bool, int, symbol, string, keyword) are always
    /// sendable.
    pub fn is_sendable(&self) -> bool {
        match self {
            TableKey::Heap(_) => false,
            TableKey::Array(keys) => keys.iter().all(|k| k.is_sendable()),
            _ => true,
        }
    }

    fn discriminant_index(&self) -> u8 {
        match self {
            TableKey::Nil => 0,
            TableKey::Bool(_) => 1,
            TableKey::Int(_) => 2,
            TableKey::Symbol(_) => 3,
            TableKey::String(_) => 4,
            TableKey::Keyword(_) => 5,
            TableKey::EmptyList => 6,
            TableKey::Array(_) => 7,
            TableKey::Heap(_) => 8,
        }
    }
}

impl std::hash::Hash for TableKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            TableKey::Nil => {}
            TableKey::EmptyList => {}
            TableKey::Bool(b) => b.hash(state),
            TableKey::Int(i) => i.hash(state),
            TableKey::Symbol(id) => id.hash(state),
            TableKey::String(s) => s.hash(state),
            TableKey::Keyword(s) => s.hash(state),
            TableKey::Array(keys) => keys.hash(state),
            // Delegate to Value's Hash. For Fiber/ThreadHandle/External
            // that hashes the backing Rc/Arc rather than the slot pointer,
            // so a `with-traits` wrapper is the same map key as the value
            // it wraps (see `repr/eq.rs`, "Wrapper variants take their
            // identity from the handle"). For cons/set/struct/bytes/
            // empty-list, gives structural hashing based on the content.
            TableKey::Heap(v) => v.hash(state),
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
            (TableKey::EmptyList, TableKey::EmptyList) => true,
            (TableKey::Array(a), TableKey::Array(b)) => a == b,
            // Delegate to Value's PartialEq (stable identity for Fiber
            // and friends — see Hash impl above). Structural equality
            // for cons/set/struct/bytes/empty-list.
            (TableKey::Heap(a), TableKey::Heap(b)) => a == b,
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
        // Discriminant index: Nil=0, Bool=1, Int=2, Symbol=3, String=4, Keyword=5, EmptyList=6, Array=7, Heap=8
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
            (TableKey::EmptyList, TableKey::EmptyList) => std::cmp::Ordering::Equal,
            (TableKey::Array(a), TableKey::Array(b)) => a.cmp(b),
            // Delegate to Value's Ord. Stable identity for Fiber and
            // friends — see Hash impl above. Structural ordering for
            // cons/set/struct/bytes/empty-list.
            (TableKey::Heap(a), TableKey::Heap(b)) => a.cmp(b),
            _ => unreachable!("discriminant match already handled"),
        }
    }
}

/// Render a `TableKey` — the shared body for `Display` (`debug == false`) and
/// `Debug` (`debug == true`). A symbol key's name comes from the process-global
/// registry (docs/impl/symbol.md), so nothing is threaded. The two modes diverge
/// only in the symbol arm (Display prints the raw `SymbolId`; Debug prints the
/// quoted name) and in the nested recursion of array/heap keys, which follows the
/// outer mode.
pub(crate) fn fmt_table_key(
    key: &TableKey,
    symbols: Option<&crate::symbol::SymbolTable>,
    debug: bool,
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    match key {
        TableKey::Nil => write!(f, "nil"),
        TableKey::Bool(b) => write!(f, "{}", b),
        TableKey::Int(i) => write!(f, "{}", i),
        TableKey::Symbol(id) => {
            if debug {
                match symbols.and_then(|s| s.name(*id)) {
                    Some(name) => write!(f, "'{}", name),
                    None => write!(f, "'#<symbol:{:#x}>", id.0),
                }
            } else {
                write!(f, "{:?}", id)
            }
        }
        TableKey::String(s) => write!(f, "\"{}\"", s),
        TableKey::Keyword(hash) => {
            match crate::value::keyword::resolve_keyword_name(symbols, *hash) {
                Some(name) => write!(f, ":{}", name),
                None => write!(f, "#<keyword:{:#x}>", hash),
            }
        }
        TableKey::EmptyList => write!(f, "()"),
        TableKey::Array(keys) => {
            write!(f, "[")?;
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                fmt_table_key(k, symbols, debug, f)?;
            }
            write!(f, "]")
        }
        TableKey::Heap(v) => crate::value::display::fmt_value(v, symbols, debug, f),
    }
}

impl fmt::Display for TableKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_table_key(self, None, false, f)
    }
}

/// A `TableKey` paired with a memo for name-resolving Debug-style rendering —
/// the key analogue of `Value::debug_with`, for formatters that print keys
/// outside `fmt_value`'s recursion.
pub(crate) struct TableKeyDisplay<'a>(pub &'a TableKey, pub Option<&'a crate::symbol::SymbolTable>);

impl fmt::Display for TableKeyDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_table_key(self.0, self.1, true, f)
    }
}

impl fmt::Debug for TableKey {
    /// Machine-readable representation of table keys.
    /// Symbols: `'#<symbol:hash>` — a bare `Debug` threads no memo, so it has no
    /// name to print; `fmt_table_key` with a memo renders `'name`.
    /// Strings: "value" (with quotes)
    /// Keywords: :name
    /// Others: same as Display
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_table_key(self, None, true, f)
    }
}

// ── Sorted struct slice helpers ───────────────────────────────────────────

/// Look up a key in a sorted struct slice by binary search.
#[inline]
pub fn sorted_struct_get<'a>(
    entries: &'a [(TableKey, super::Value)],
    key: &TableKey,
) -> Option<&'a super::Value> {
    entries
        .binary_search_by(|(k, _)| k.cmp(key))
        .ok()
        .map(|i| &entries[i].1)
}

/// Check if a sorted struct slice contains a key.
#[inline]
pub fn sorted_struct_contains(entries: &[(TableKey, super::Value)], key: &TableKey) -> bool {
    entries.binary_search_by(|(k, _)| k.cmp(key)).is_ok()
}

/// Insert or update a key in a sorted Vec, maintaining sort order.
/// Returns a new Vec (for immutable struct operations).
pub fn sorted_struct_insert(
    entries: &[(TableKey, super::Value)],
    key: TableKey,
    value: super::Value,
) -> Vec<(TableKey, super::Value)> {
    let mut result = entries.to_vec();
    match result.binary_search_by(|(k, _)| k.cmp(&key)) {
        Ok(i) => result[i].1 = value,
        Err(i) => result.insert(i, (key, value)),
    }
    result
}

/// Remove a key from a sorted slice, returning a new Vec.
pub fn sorted_struct_remove(
    entries: &[(TableKey, super::Value)],
    key: &TableKey,
) -> Vec<(TableKey, super::Value)> {
    let mut result = entries.to_vec();
    if let Ok(i) = result.binary_search_by(|(k, _)| k.cmp(key)) {
        result.remove(i);
    }
    result
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
