// audited: 2026-09-21
//! Registration metadata a primitive carries beyond its call behavior: the
//! `Doc` a lookup shows, and the `PrimitiveMeta` maps the pipeline threads.
//!
//! src/primitives/AGENTS.md

use crate::primitives::def::{RegionEffect, RetType};
use crate::signals::Signal;
use crate::value::types::Arity;
use crate::value::{SymbolId, Value};
use std::collections::HashMap;

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
    /// Primitive SymbolId → declared [`RegionEffect`]. Aliases get the
    /// same entry as their primary name (a single map, so the alias
    /// metadata cannot drift). The region solver's call classification
    /// reads this.
    pub effects: HashMap<SymbolId, RegionEffect>,
    /// Primitive SymbolId → declared [`RetType`]. Aliases get the same entry.
    /// The ownership inference reads this to classify a `Funnel` store's
    /// container argument: a `MutableArray`/`MutableStruct` container *retains*
    /// the stored value's region (so the forest recovers a containment edge),
    /// where a `String`/`Unknown` (e.g. `@string`/`@bytes`) container copies
    /// bytes and retains nothing.
    pub ret_types: HashMap<SymbolId, RetType>,
    /// Primitive SymbolId → the argument indices it EMBEDS into its fresh result
    /// ([`crate::primitives::def::PrimitiveDef::embeds`]). Aliases get the same entry. The region walk's
    /// `Fresh` arm reads this (through `CallClassification::embeds`) to record a
    /// `result ⊇ arg` containment edge for each embedded argument.
    pub embeds: HashMap<SymbolId, &'static [usize]>,
    /// Primitive SymbolId → [`crate::primitives::def::PrimitiveDef::moves_out`]. Aliases get the same entry.
    /// A moves-out native's heap result is an element REMOVED from a container arg,
    /// escape-retained IN-BODY before the container release (`%pop`); the region
    /// walk reads this (through `CallClassification::moves_out`) to suppress the
    /// redundant tail ReturnValue retain — but only when the effect is also
    /// `PassThrough` (a genuinely non-fresh move-out), so a fresh grapheme/byte
    /// result keeps its retain.
    pub moves_out: HashMap<SymbolId, bool>,
}

impl PrimitiveMeta {
    pub fn new() -> Self {
        PrimitiveMeta {
            signals: HashMap::new(),
            arities: HashMap::new(),
            docs: HashMap::new(),
            functions: HashMap::new(),
            effects: HashMap::new(),
            ret_types: HashMap::new(),
            embeds: HashMap::new(),
            moves_out: HashMap::new(),
        }
    }
}

impl Default for PrimitiveMeta {
    fn default() -> Self {
        Self::new()
    }
}
