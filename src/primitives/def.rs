//! Primitive definition type for declarative registration.
//!
//! Each primitive module exports a `const PRIMITIVES: &[PrimitiveDef]`
//! table. `register_primitives` iterates all tables to register
//! primitives with the VM and build the metadata maps.

use crate::effects::Effect;
use crate::value::types::{Arity, NativeFn};
use crate::value::SymbolId;
use std::collections::HashMap;

/// Declarative definition of a primitive function.
///
/// All metadata for a primitive lives here. Each primitive module
/// exports a const array of these. Adding a new metadata field
/// means adding it here with a default; existing tables use
/// `..PrimitiveDef::DEFAULT`.
pub struct PrimitiveDef {
    /// The Elle-facing name (e.g., "math/sin", "cons").
    pub name: &'static str,
    /// The Rust implementation.
    pub func: NativeFn,
    /// Signal effects (raises, yields, etc.).
    pub effect: Effect,
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
}

impl PrimitiveDef {
    /// Default for struct-update syntax. Intentionally panics at
    /// runtime if `func` is called — forces explicit initialization.
    pub const DEFAULT: PrimitiveDef = PrimitiveDef {
        name: "",
        func: _default_prim,
        effect: Effect::none(),
        arity: Arity::Exact(0),
        doc: "",
        params: &[],
        category: "",
        example: "",
        aliases: &[],
    };
}

/// Placeholder function for DEFAULT — should never be called.
const fn _default_prim(
    _args: &[crate::value::Value],
) -> (crate::value::fiber::SignalBits, crate::value::Value) {
    panic!("PrimitiveDef::DEFAULT func called — this is a bug")
}

/// Documentation info for a primitive, stored at runtime for `doc` lookup.
#[derive(Debug, Clone)]
pub struct PrimitiveDoc {
    pub name: &'static str,
    pub doc: &'static str,
    pub params: &'static [&'static str],
    pub arity: Arity,
    pub effect: Effect,
    pub category: &'static str,
    pub example: &'static str,
}

impl PrimitiveDoc {
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
        out
    }
}

/// Metadata extracted from primitive registration.
///
/// Returned by `register_primitives` and threaded through the
/// pipeline to the analyzer. Single source of truth for all
/// primitive metadata.
pub struct PrimitiveMeta {
    pub effects: HashMap<SymbolId, Effect>,
    pub arities: HashMap<SymbolId, Arity>,
    pub docs: HashMap<SymbolId, PrimitiveDoc>,
}

impl PrimitiveMeta {
    pub fn new() -> Self {
        PrimitiveMeta {
            effects: HashMap::new(),
            arities: HashMap::new(),
            docs: HashMap::new(),
        }
    }
}

impl Default for PrimitiveMeta {
    fn default() -> Self {
        Self::new()
    }
}
