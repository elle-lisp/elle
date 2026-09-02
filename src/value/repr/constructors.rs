//! Value constructors for immediate (non-heap) types.
//!
//! Heap values are built through the region-and-heap-explicit
//! `crate::value::build::*` source (and the ergonomic `NativeCtx`/`Alloc`
//! `ctx.*` surface that forwards to it). There is no heap-free `Value::*`
//! heap constructor: a heap allocation names the heap it is born on or it does
//! not compile (tls.md § "The honesty invariant").

use super::{Value, TAG_CPOINTER, TAG_FLOAT, TAG_INT, TAG_KEYWORD, TAG_SYMBOL};

impl Value {
    // =========================================================================
    // Immediate Value Constructors
    // =========================================================================

    /// Create an integer value.
    #[inline]
    pub fn int(n: i64) -> Self {
        Value {
            tag: TAG_INT,
            payload: n as u64,
        }
    }

    /// Create a float value.
    #[inline]
    pub fn float(f: f64) -> Self {
        Value {
            tag: TAG_FLOAT,
            payload: f.to_bits(),
        }
    }

    /// Create a boolean value.
    #[inline]
    pub fn bool(b: bool) -> Self {
        if b {
            Self::TRUE
        } else {
            Self::FALSE
        }
    }

    /// Create a symbol value from a `SymbolId`.
    ///
    /// Takes the newtype, not a bare `u64`: a keyword hash is also a `u64`, and
    /// the two payloads would otherwise be swappable at a call site with no
    /// compile error.
    #[inline]
    pub fn symbol(id: crate::value::SymbolId) -> Self {
        Value {
            tag: TAG_SYMBOL,
            payload: id.0,
        }
    }

    /// Create a keyword value from a name string. Identity only — the
    /// spelling is recorded nowhere; display resolves it through the
    /// per-instance memo and the static vocabulary
    /// (docs/impl/symbol.md § "The display memo"). `const`, so a well-known
    /// keyword can be a constant.
    #[inline]
    pub const fn keyword(name: &str) -> Self {
        Value {
            tag: TAG_KEYWORD,
            payload: crate::value::keyword::keyword_hash(name),
        }
    }

    /// Rebuild a keyword from its payload — a hash that came off an existing
    /// keyword (`keyword_hash()`), a `TableKey::Keyword`, or a decoded
    /// constant. Not for arbitrary integers: a payload that never came from
    /// [`keyword_hash`](crate::value::keyword::keyword_hash) names nothing.
    #[inline]
    pub(crate) fn keyword_from_hash(hash: u64) -> Self {
        Value {
            tag: TAG_KEYWORD,
            payload: hash,
        }
    }

    /// Create a raw C pointer value.
    ///
    /// NULL pointers (address 0) are represented as `Value::NIL`.
    /// This is an immediate value, not heap-allocated.
    #[inline]
    pub fn pointer(addr: usize) -> Self {
        if addr == 0 {
            return Self::NIL;
        }
        Value {
            tag: TAG_CPOINTER,
            payload: addr as u64,
        }
    }

    /// Create an empty list value.
    #[inline]
    pub fn empty_list() -> Self {
        Self::EMPTY_LIST
    }

    /// Create a native function value from a static primitive definition.
    ///
    /// A native-fn is an IMMEDIATE: `Value{TAG_NATIVE_FN, prim_id}`, where the
    /// prim_id is the def's dense index in `primitives::table()`. It carries no
    /// heap payload and no region — a native-fn is a `&'static PrimitiveDef`,
    /// nothing to reclaim. Identity is by prim_id (the immediate eq path),
    /// portable across `send`/`spawn`, and the switch key for tier-2 lowering.
    #[inline]
    pub fn native_fn(def: &'static crate::primitives::def::PrimitiveDef) -> Self {
        Value {
            tag: super::TAG_NATIVE_FN,
            payload: crate::primitives::prim_id_of(def) as u64,
        }
    }
}
