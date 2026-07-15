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

pub(crate) mod convert;
mod display;
mod expand;
mod span;

pub use expand::{Expander, MacroDef};
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

/// Pre-analysis syntax tree node.
#[derive(Debug, Clone)]
pub struct Syntax {
    pub kind: SyntaxKind,
    pub span: Span,
    /// Scope set for hygiene. Two identifiers match only if their
    /// scope sets are compatible (implementation: subset check).
    pub(crate) scopes: Vec<ScopeId>,
    /// When true, `add_scope_recursive` skips this node and its children.
    /// Set by `datum->syntax` to prevent the intro scope from being added
    /// to nodes that should resolve at the call site (hygiene escape hatch).
    /// Only affects `add_scope_recursive`, not `add_scope`.
    pub scope_exempt: bool,
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

impl Syntax {
    /// Create a new Syntax node with empty scope set
    pub fn new(kind: SyntaxKind, span: Span) -> Self {
        Syntax {
            kind,
            span,
            scopes: Vec::new(),
            scope_exempt: false,
        }
    }

    /// Create a new Syntax node with given scope set
    pub(crate) fn with_scopes(kind: SyntaxKind, span: Span, scopes: Vec<ScopeId>) -> Self {
        Syntax {
            kind,
            span,
            scopes,
            scope_exempt: false,
        }
    }

    /// Add a scope to this node's scope set
    pub(crate) fn add_scope(&mut self, scope: ScopeId) {
        if !self.scopes.contains(&scope) {
            self.scopes.push(scope);
        }
    }

    /// Flip `scope` on this node: remove it if present, add it if absent.
    /// The macro-expansion hygiene operation (Flatt's sets-of-scopes):
    /// applied to everything a transformer returns, it MARKS
    /// template-origin identifiers (which never saw the intro scope) and
    /// UNMARKS argument-origin identifiers (pre-stamped before the call),
    /// distinguishing the two without tracking provenance.
    pub(crate) fn flip_scope(&mut self, scope: ScopeId) {
        if let Some(pos) = self.scopes.iter().position(|s| *s == scope) {
            self.scopes.remove(pos);
        } else {
            self.scopes.push(scope);
        }
    }

    /// Replace the scope set on this node and all children with the given
    /// scopes, and mark all nodes as scope-exempt. Used by `datum->syntax`
    /// to give a datum the lexical context of another syntax object while
    /// preventing `add_scope_recursive` from overriding those scopes.
    pub(crate) fn set_scopes_recursive(&mut self, scopes: &[ScopeId]) {
        self.scopes = scopes.to_vec();
        self.scope_exempt = true;
        match &mut self.kind {
            SyntaxKind::List(items)
            | SyntaxKind::Array(items)
            | SyntaxKind::ArrayMut(items)
            | SyntaxKind::Struct(items)
            | SyntaxKind::StructMut(items)
            | SyntaxKind::Set(items)
            | SyntaxKind::SetMut(items)
            | SyntaxKind::Bytes(items)
            | SyntaxKind::BytesMut(items) => {
                for item in items {
                    item.set_scopes_recursive(scopes);
                }
            }
            SyntaxKind::Quote(inner) => {
                inner.set_scopes_recursive(scopes);
            }
            SyntaxKind::Quasiquote(inner)
            | SyntaxKind::Unquote(inner)
            | SyntaxKind::UnquoteSplicing(inner)
            | SyntaxKind::Splice(inner) => {
                inner.set_scopes_recursive(scopes);
            }
            // Atoms don't have children to recurse into
            SyntaxKind::Nil
            | SyntaxKind::Bool(_)
            | SyntaxKind::Int(_)
            | SyntaxKind::Float(_)
            | SyntaxKind::Symbol(_)
            | SyntaxKind::Keyword(_)
            | SyntaxKind::String(_)
            | SyntaxKind::StringMut(_) => {}
            // SyntaxLiteral is internal-only (created by expand_macro_call_inner);
            // it should never appear in datum->syntax input from from_value()
            SyntaxKind::SyntaxLiteral(_) => {}
        }
    }

    /// Check if this is a symbol with the given name
    pub fn is_symbol(&self, name: &str) -> bool {
        matches!(&self.kind, SyntaxKind::Symbol(s) if s == name)
    }

    /// Get symbol name if this is a symbol
    pub fn as_symbol(&self) -> Option<&str> {
        match &self.kind {
            SyntaxKind::Symbol(s) => Some(s),
            _ => None,
        }
    }

    /// Get list contents if this is a list
    pub fn as_list(&self) -> Option<&[Syntax]> {
        match &self.kind {
            SyntaxKind::List(items) => Some(items),
            _ => None,
        }
    }

    /// Get contents if this is a list or array.
    ///
    /// Structural positions in special forms (params, bindings, clauses,
    /// arms) accept both `(...)` and `[...]`. Expression-position uses
    /// of `[...]` remain array literals.
    pub fn as_list_or_tuple(&self) -> Option<&[Syntax]> {
        match &self.kind {
            SyntaxKind::List(items) | SyntaxKind::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Human-readable label for the syntax kind, used in error messages.
    pub fn kind_label(&self) -> &'static str {
        match &self.kind {
            SyntaxKind::Nil => "nil",
            SyntaxKind::Bool(_) => "boolean",
            SyntaxKind::Int(_) => "integer",
            SyntaxKind::Float(_) => "float",
            SyntaxKind::Symbol(_) => "symbol",
            SyntaxKind::Keyword(_) => "keyword",
            SyntaxKind::String(_) => "string",
            SyntaxKind::StringMut(_) => "@string",
            SyntaxKind::List(_) => "list",
            SyntaxKind::Array(_) => "array",
            SyntaxKind::ArrayMut(_) => "@array",
            SyntaxKind::Struct(_) => "struct",
            SyntaxKind::StructMut(_) => "@struct",
            SyntaxKind::Set(_) => "set",
            SyntaxKind::SetMut(_) => "mutable set",
            SyntaxKind::Bytes(_) => "bytes",
            SyntaxKind::BytesMut(_) => "@bytes",
            SyntaxKind::Quote(_) => "quote",
            SyntaxKind::Quasiquote(_) => "quasiquote",
            SyntaxKind::Unquote(_) => "unquote",
            SyntaxKind::UnquoteSplicing(_) => "unquote-splicing",
            SyntaxKind::Splice(_) => "splice",
            SyntaxKind::SyntaxLiteral(_) => "syntax-literal",
        }
    }
}

#[derive(Debug, Clone)]
pub enum SyntaxKind {
    // Atoms
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Symbol(String),
    Keyword(String),
    String(String),
    /// Mutable string literal: `@"..."`
    StringMut(String),

    // Compounds
    List(Vec<Syntax>),
    /// Bracket-delimited immutable array: `[...]`
    Array(Vec<Syntax>),
    /// Bracket-delimited mutable array: `@[...]`
    ArrayMut(Vec<Syntax>),
    /// Brace-delimited immutable struct: `{...}`
    Struct(Vec<Syntax>),
    /// Brace-delimited mutable struct: `@{...}`
    StructMut(Vec<Syntax>),
    /// Pipe-delimited immutable set literal: `|...|`
    Set(Vec<Syntax>),
    /// Pipe-delimited mutable set literal: `@|...|`
    SetMut(Vec<Syntax>),
    /// Bytes literal: `b[...]`
    Bytes(Vec<Syntax>),
    /// Mutable bytes literal: `@b[...]`
    BytesMut(Vec<Syntax>),

    // Quote forms - preserved as structure for macro handling
    Quote(Box<Syntax>),
    Quasiquote(Box<Syntax>),
    Unquote(Box<Syntax>),
    UnquoteSplicing(Box<Syntax>),
    /// Splice form: `;expr` or `(splice expr)`. Marks a value for
    /// array-spreading at call sites and data constructors.
    Splice(Box<Syntax>),

    /// Internal: a hygiene-bearing template symbol carried as plain compile-time
    /// data (NOT a pre-baked heap `Value`). Never produced by the reader; created
    /// by quasiquote (`quasiquote_to_code`) to preserve a template symbol's scope
    /// set through the expansion round-trip. The analyzer materializes it as an
    /// ORDINARY allocation per execution via `HirKind::QuoteConst`
    /// (`ConstTemplate::SyntaxSymbol`) — a heap literal is an ordinary,
    /// reclaimable allocation.
    SyntaxLiteral(std::rc::Rc<Syntax>),
}

#[cfg(test)]
mod tests;
