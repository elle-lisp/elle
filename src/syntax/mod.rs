//! Syntax tree representation for Elle source code
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
//! therefore names a [`SyntaxArena`]. See docs/impl/syntax.md.

mod arena;
pub(crate) mod convert;
mod display;
mod expand;
pub mod files;
mod node;
mod span;

pub use arena::{thread_arena, SyntaxArena, SyntaxHeap};
pub use expand::{Expander, MacroDef};
pub use node::{SeqCtor, SynRef, Syntax, SyntaxKind, WrapCtor};
pub use span::Span;

/// Unique identifier for a lexical scope.
/// Used for hygienic macro expansion - identifiers with different scope sets
/// are considered different even if they have the same name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ScopeId(pub u32);

impl ScopeId {
    /// Reserved bit marking a macro-expansion INTRO scope (minted per
    /// expansion, flipped onto template-origin nodes). Carrying intro-ness
    /// in the id itself lets the Analyzer apply the referential-transparency
    /// rule (`hir::analyze::scopes::lookup`) without threading expander state.
    const INTRO_BIT: u32 = 1 << 31;

    /// Mint the intro-scope id for counter value `n`.
    pub(crate) fn intro(n: u32) -> ScopeId {
        ScopeId(Self::INTRO_BIT | n)
    }

    /// Is this a macro-expansion intro scope?
    pub(crate) fn is_intro(self) -> bool {
        self.0 & Self::INTRO_BIT != 0
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
mod tests;
