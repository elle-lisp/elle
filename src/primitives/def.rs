//! Primitive definition type for declarative registration.
//!
//! Each primitive module exports a `const PRIMITIVES: &[PrimitiveDef]`
//! table. `register_primitives` iterates all tables to register
//! primitives with the VM and build the metadata maps.

use crate::signals::Signal;
use crate::value::types::{Arity, PrimFn};
use crate::value::{SymbolId, Value};
use rustc_hash::FxHashSet;
use std::collections::HashMap;

/// Whether a primitive returns a heap-allocated value or an immediate.
///
/// Used by the region solver to skip phantom allocation vars for calls
/// that never allocate (predicates, arithmetic, comparisons, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnType {
    /// May return a heap-allocated value (default, conservative).
    Heap,
    /// Always returns an immediate (bool, int, float, keyword, nil).
    Immediate,
}

/// Declarative definition of a primitive function.
///
/// All metadata for a primitive lives here. Each primitive module
/// exports a const array of these. Adding a new metadata field
/// means adding it here with a default; existing tables use
/// `..PrimitiveDef::DEFAULT`.
pub struct PrimitiveDef {
    /// The Elle-facing name (e.g., "math/sin", "pair").
    pub name: &'static str,
    /// The Rust implementation.
    pub func: PrimFn,
    /// Signal (errors, yields, etc.).
    pub signal: Signal,
    /// Argument count constraint.
    pub arity: Arity,
    /// One-line description for help/hover/docs.
    pub doc: &'static str,
    /// Parameter names for signature help.
    /// Empty slice for nullary or variadic-only functions.
    pub params: &'static [&'static str],
    /// Module/category (e.g., "math", "string", "file").
    /// Empty string for core (unprefixed) primitives.
    pub category: &'static str,
    /// Runnable example in Elle syntax. Picked up by elle-doc.
    /// Empty string if no example.
    pub example: &'static str,
    /// Aliases — additional names that resolve to the same function.
    /// Registered with identical metadata.
    pub aliases: &'static [&'static str],
    /// Whether this primitive returns a heap-allocated value or an immediate.
    pub returns: ReturnType,
}

impl PrimitiveDef {
    /// Default for struct-update syntax. Intentionally panics at
    /// runtime if `func` is called — forces explicit initialization.
    pub const DEFAULT: PrimitiveDef = PrimitiveDef {
        name: "",
        func: _default_prim,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "",
        params: &[],
        category: "",
        example: "",
        aliases: &[],
        returns: ReturnType::Heap,
    };
}

/// Placeholder function for DEFAULT — should never be called.
const fn _default_prim(
    _args: &[crate::value::Value],
) -> (crate::value::fiber::SignalBits, crate::value::Value) {
    panic!("PrimitiveDef::DEFAULT func called — this is a bug")
}

// Re-export for use in primitive modules.
pub use ReturnType::Immediate as IMMEDIATE;

/// Declare a `pub(crate) const PRIMITIVES: &[PrimitiveDef]` table.
///
/// Each entry is `"name" => func_name { key: value, ... }`.
/// Only `name` and `func` are required; all other fields default to
/// the values in `PrimitiveDef::DEFAULT`.
///
/// ```ignore
/// primitive! {
///     "math/sqrt" => prim_sqrt {
///         signal: Signal::errors(), arity: Arity::Exact(1),
///         doc: "Square root.", params: &["x"],
///         category: "math", example: "(math/sqrt 16)",
///         aliases: &["sqrt"],
///     }
///     "and" => prim_and {
///         arity: Arity::AtLeast(0),
///         doc: "Logical AND.",
///         category: "logic", example: "(and true false)",
///     }
/// }
/// ```
macro_rules! primitive {
    ( $( $name:literal => $func:ident { $( $key:ident : $val:expr ),* $(,)? } )* ) => {
        pub(crate) const PRIMITIVES: &[crate::primitives::def::PrimitiveDef] = &[
            $(
                crate::primitives::def::PrimitiveDef {
                    name: $name,
                    func: $func,
                    $( $key : $val, )*
                    ..crate::primitives::def::PrimitiveDef::DEFAULT
                }
            ),*
        ];
    };
}

/// No-op primitive: returns (SIG_OK, nil). Used by ad-hoc Value::native_fn
/// creation in tests and FFI wrappers.
fn _noop_prim(
    _args: &[crate::value::Value],
) -> (crate::value::fiber::SignalBits, crate::value::Value) {
    (
        crate::value::fiber::SignalBits::EMPTY,
        crate::value::Value::NIL,
    )
}

/// A silent, no-op PrimitiveDef for ad-hoc `Value::native_fn` creation.
/// Used by tests and FFI wrappers that need a `&'static PrimitiveDef`.
pub static NOOP_PRIM: PrimitiveDef = PrimitiveDef {
    name: "<noop>",
    func: _noop_prim,
    signal: Signal::silent(),
    arity: Arity::AtLeast(0),
    doc: "",
    params: &[],
    category: "",
    example: "",
    aliases: &[],
    returns: ReturnType::Heap,
};

/// Documentation info for a named form (primitive, special form, or macro).
/// Stored at runtime for `doc` lookup.
#[derive(Debug, Clone)]
pub struct Doc {
    pub name: &'static str,
    pub doc: &'static str,
    pub params: &'static [&'static str],
    pub arity: Arity,
    pub signal: Signal,
    pub category: &'static str,
    pub example: &'static str,
    pub aliases: &'static [&'static str],
}

impl Doc {
    /// Format as a human-readable doc string for REPL display.
    pub fn format(&self) -> String {
        let mut out = String::new();
        // Signature line
        out.push('(');
        out.push_str(self.name);
        for p in self.params {
            out.push(' ');
            out.push_str(p);
        }
        out.push(')');
        out.push('\n');
        // Description
        if !self.doc.is_empty() {
            out.push_str("  ");
            out.push_str(self.doc);
            out.push('\n');
        }
        // Arity
        out.push_str("  arity: ");
        out.push_str(&format!("{}", self.arity));
        out.push('\n');
        // Example
        if !self.example.is_empty() {
            out.push_str("  example:\n");
            for line in self.example.lines() {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
        }
        // Aliases
        if !self.aliases.is_empty() {
            out.push_str("  aliases: ");
            out.push_str(&self.aliases.join(", "));
            out.push('\n');
        }
        out
    }
}

/// Metadata extracted from primitive registration.
///
/// Returned by `register_primitives` and threaded through the
/// pipeline to the analyzer. Single source of truth for all
/// primitive metadata.
#[derive(Clone)]
pub struct PrimitiveMeta {
    pub signals: HashMap<SymbolId, Signal>,
    pub arities: HashMap<SymbolId, Arity>,
    pub docs: HashMap<SymbolId, Doc>,
    /// NativeFn values for each primitive, keyed by SymbolId.
    /// Used by `bind_primitives` to record compile-time constant
    /// values so the lowerer can emit `LoadConst` instead of
    /// `LoadGlobal`.
    pub functions: HashMap<SymbolId, Value>,
    /// Primitives that always return immediates (bool, int, float, keyword, nil).
    /// Used by the region solver to skip phantom allocation vars.
    pub immediate_primitives: FxHashSet<SymbolId>,
}

impl PrimitiveMeta {
    pub fn new() -> Self {
        PrimitiveMeta {
            signals: HashMap::new(),
            arities: HashMap::new(),
            docs: HashMap::new(),
            functions: HashMap::new(),
            immediate_primitives: FxHashSet::default(),
        }
    }
}

impl Default for PrimitiveMeta {
    fn default() -> Self {
        Self::new()
    }
}
