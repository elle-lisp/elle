// audited: 2026-10-13
//! Syntax tree representation for Elle source code
//!
//! docs/impl/syntax.md
//!
//! This module provides the pre-analysis AST representation. Unlike `Value`,
//! which is the runtime representation, `Syntax` is specifically designed for:
//! - Preserving source locations
//! - Supporting hygienic macro expansion via scope sets
//! - Deferring symbol interning until analysis
//!
//! The compilation pipeline is:
//! ```text
//! Source → Lexer → Token → Parser → Syntax → Expand → Syntax → Analyze → HIR
//! ```
//!
//! The tree is region data: nodes, child slices, and string payloads live in
//! region pages, and a node is `Copy` POD with no `Drop`. Every constructor
//! therefore names a [`SyntaxArena`].

mod arena;
pub(crate) mod convert;
mod display;
mod expand;
pub mod files;
mod kind;
mod node;
mod span;

pub use arena::{thread_arena, SyntaxArena, SyntaxHeap};
pub use expand::{Expander, MacroDef, MacroParams};
pub use kind::{SeqCtor, SyntaxKind, WrapCtor};
pub use node::{SynRef, Syntax};
pub use span::Span;

/// The unit of expansion a scope id was minted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct UnitTag(u32);

impl UnitTag {
    /// The tag no unit takes: the primitive scope and the compiler's intro
    /// scope carry it.
    pub(crate) const NONE: UnitTag = UnitTag(0);
}

/// A hygiene scope. Identifiers with the same name and different scope sets
/// are different identifiers.
///
/// The id packs the minting unit's tag above the intro bit and a count within
/// the unit. The constructors and accessors below are the only code that knows
/// the packing.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct ScopeId(u64);

impl ScopeId {
    /// Marks a macro-expansion intro scope, which an expansion flips onto its
    /// template-origin nodes. The analyzer's referential-transparency rule
    /// (`hir::analyze::scopes::lookup`) reads it off the id.
    const INTRO_BIT: u64 = 1 << 31;

    /// The bits of the count within the unit.
    const COUNT_MASK: u64 = Self::INTRO_BIT - 1;

    /// Where the unit's tag starts.
    const UNIT_SHIFT: u32 = 32;

    /// The scope every prelude form and every primitive binding carries, so a
    /// prelude template's symbols resolve to primitives whatever the use site
    /// binds.
    pub(crate) const PRIMITIVE: ScopeId = ScopeId(0);

    /// The intro scope the analyzer stamps on a form it builds itself, such as
    /// the load a computed `import-file` becomes. Its symbols then skip
    /// use-site locals and resolve at top level, as a macro template's do. It
    /// takes the last count under no unit's tag, so no expansion mints it.
    pub(crate) const COMPILER: ScopeId = ScopeId(Self::INTRO_BIT | Self::COUNT_MASK);

    fn pack(unit: UnitTag, intro: bool, count: u32) -> ScopeId {
        let count = count as u64;
        assert!(
            count <= Self::COUNT_MASK,
            "a unit minted more than 2^31 hygiene scopes"
        );
        let intro = if intro { Self::INTRO_BIT } else { 0 };
        ScopeId((unit.0 as u64) << Self::UNIT_SHIFT | intro | count)
    }

    /// The ordinary scope `count` of `unit`.
    pub(crate) fn new(unit: UnitTag, count: u32) -> ScopeId {
        Self::pack(unit, false, count)
    }

    /// The intro scope `count` of `unit`.
    pub(crate) fn intro(unit: UnitTag, count: u32) -> ScopeId {
        Self::pack(unit, true, count)
    }

    /// Is this a macro-expansion intro scope?
    pub(crate) fn is_intro(self) -> bool {
        self.0 & Self::INTRO_BIT != 0
    }

    /// The count within the minting unit, without the intro bit.
    pub(crate) fn count(self) -> u32 {
        (self.0 & Self::COUNT_MASK) as u32
    }

    /// The id as one word, for an encoding that writes it out.
    pub(crate) fn to_bits(self) -> u64 {
        self.0
    }

    /// The id a word from [`to_bits`](Self::to_bits) names.
    pub(crate) fn from_bits(bits: u64) -> ScopeId {
        ScopeId(bits)
    }
}

#[cfg(test)]
impl ScopeId {
    /// The ordinary scope `count` under no unit's tag, for a test that needs
    /// distinct ids and not the unit they came from.
    pub(crate) fn test(count: u32) -> ScopeId {
        Self::new(UnitTag::NONE, count)
    }
}

thread_local! {
    /// The intro scope of the macro expansion currently running its
    /// transformer, if any. Set (and restored) by `expand_macro_call_inner`
    /// around the transformer call; read by `prim_datum_to_syntax` to strip
    /// the pre-stamped intro scope from copied context scopes, so
    /// datum->syntax results carry the context's true use-site scope set.
    static CURRENT_MACRO_INTRO: std::cell::Cell<Option<ScopeId>> =
        const { std::cell::Cell::new(None) };
}

/// Swap the currently-running macro expansion's intro scope, returning the
/// previous value (save/restore discipline for nested expansions).
pub(crate) fn set_current_macro_intro(scope: Option<ScopeId>) -> Option<ScopeId> {
    CURRENT_MACRO_INTRO.with(|cell| cell.replace(scope))
}

/// The intro scope of the macro expansion currently running its transformer.
pub(crate) fn current_macro_intro() -> Option<ScopeId> {
    CURRENT_MACRO_INTRO.with(|cell| cell.get())
}

/// Is `name` the rest-collector marker? `&rest` is a synonym for `&` in
/// every rest-collector position — function parameter lists, destructuring
/// patterns (list / array / struct), `match` patterns, and `defmacro`
/// parameter lists. The ONE recognition point: every consumer (the
/// analyzer's destructure/pattern code, the macro expander) must call this
/// rather than compare against a spelling, so the synonyms cannot drift.
pub(crate) fn is_rest_marker(name: &str) -> bool {
    name == "&" || name == "&rest"
}

#[cfg(test)]
pub(crate) mod tests;
