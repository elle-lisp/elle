//! The syntax node: a region-resident, `Copy` tree node.
//!
//! docs/impl/syntax.md § "The node" owns the model. Every payload here is a
//! region handle that dereferences to an ordinary Rust view — a `&str` for a
//! name, a `&[Syntax]` for children, a `&Syntax` for a wrapped form — so a
//! reader of the tree matches and walks it without knowing about regions.

use crate::value::region_slice::{RegionSlice, RegionStr};

use super::{ScopeId, Span, SyntaxArena};

/// A pointer to one node owned by a region: the payload of the single-child
/// kinds (`Quote`, `Unquote`, …).
///
/// A one-element `RegionSlice<Syntax>` would say the same thing in 16 bytes;
/// this says it in 8, which is what keeps `SyntaxKind` at 24.
#[derive(Clone, Copy)]
pub struct SynRef(*const Syntax);

impl SynRef {
    /// # Safety
    /// `ptr` must name one live `Syntax` owned by a region that outlives every
    /// use of this reference.
    pub(crate) unsafe fn from_raw(ptr: *const Syntax) -> Self {
        SynRef(ptr)
    }

    pub(crate) fn as_ptr(self) -> *const Syntax {
        self.0
    }
}

impl std::ops::Deref for SynRef {
    type Target = Syntax;
    #[inline]
    fn deref(&self) -> &Syntax {
        unsafe { &*self.0 }
    }
}

impl std::fmt::Debug for SynRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&**self, f)
    }
}

/// Pre-analysis syntax tree node. `Copy` POD — see the module docs.
#[derive(Debug, Clone, Copy)]
pub struct Syntax {
    pub kind: SyntaxKind,
    pub span: Span,
    /// Scope set for hygiene. Two identifiers match only if their
    /// scope sets are compatible (implementation: subset check).
    pub(crate) scopes: RegionSlice<ScopeId>,
    /// When true, `add_scope_recursive` skips this node and its children.
    /// Set by `datum->syntax` to prevent the intro scope from being added
    /// to nodes that should resolve at the call site (hygiene escape hatch).
    /// Only affects `add_scope_recursive`, not `add_scope`.
    pub scope_exempt: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum SyntaxKind {
    // Atoms
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Symbol(RegionStr),
    Keyword(RegionStr),
    String(RegionStr),
    /// Mutable string literal: `@"..."`
    StringMut(RegionStr),

    // Compounds
    List(RegionSlice<Syntax>),
    /// Bracket-delimited immutable array: `[...]`
    Array(RegionSlice<Syntax>),
    /// Bracket-delimited mutable array: `@[...]`
    ArrayMut(RegionSlice<Syntax>),
    /// Brace-delimited immutable struct: `{...}`
    Struct(RegionSlice<Syntax>),
    /// Brace-delimited mutable struct: `@{...}`
    StructMut(RegionSlice<Syntax>),
    /// Pipe-delimited immutable set literal: `|...|`
    Set(RegionSlice<Syntax>),
    /// Pipe-delimited mutable set literal: `@|...|`
    SetMut(RegionSlice<Syntax>),
    /// Bytes literal: `b[...]`
    Bytes(RegionSlice<Syntax>),
    /// Mutable bytes literal: `@b[...]`
    BytesMut(RegionSlice<Syntax>),

    // Quote forms - preserved as structure for macro handling
    Quote(SynRef),
    Quasiquote(SynRef),
    Unquote(SynRef),
    UnquoteSplicing(SynRef),
    /// Splice form: `;expr` or `(splice expr)`. Marks a value for
    /// array-spreading at call sites and data constructors.
    Splice(SynRef),

    /// Internal: a hygiene-bearing template symbol carried as plain compile-time
    /// data (NOT a pre-baked heap `Value`). Never produced by the reader; created
    /// by quasiquote (`quasiquote_to_code`) to preserve a template symbol's scope
    /// set through the expansion round-trip. The analyzer materializes it as an
    /// ORDINARY allocation per execution via `HirKind::QuoteConst`
    /// (`ConstTemplate::SyntaxSymbol`) — a heap literal is an ordinary,
    /// reclaimable allocation.
    SyntaxLiteral(SynRef),
}

/// The constructor of a sequence kind: `SyntaxKind::List` and its eight
/// siblings all have this signature, so a caller can name one by value.
pub type SeqCtor = fn(RegionSlice<Syntax>) -> SyntaxKind;

/// The constructor of a single-child kind: `SyntaxKind::Quote` and its
/// siblings.
pub type WrapCtor = fn(SynRef) -> SyntaxKind;

/// How a compound kind holds its children — the shape a generic walk needs in
/// order to take a tree apart and put an equivalent one back together.
///
/// The nine sequence kinds and the six single-child kinds each differ only in
/// their tag, so every recursive pass over the tree (deep copy, scope
/// stamping, keyword learning, the `SyntaxLiteral` scan) would otherwise
/// repeat a fifteen-arm match. [`SyntaxKind::children`] and
/// [`SyntaxKind::rebuild`] are that match, written once.
enum Shape {
    Atom,
    /// A sequence kind and its constructor.
    Seq(SeqCtor),
    /// A single-child kind and its constructor.
    Wrap(WrapCtor),
}

impl SyntaxKind {
    fn shape(&self) -> Shape {
        match self {
            SyntaxKind::List(_) => Shape::Seq(SyntaxKind::List),
            SyntaxKind::Array(_) => Shape::Seq(SyntaxKind::Array),
            SyntaxKind::ArrayMut(_) => Shape::Seq(SyntaxKind::ArrayMut),
            SyntaxKind::Struct(_) => Shape::Seq(SyntaxKind::Struct),
            SyntaxKind::StructMut(_) => Shape::Seq(SyntaxKind::StructMut),
            SyntaxKind::Set(_) => Shape::Seq(SyntaxKind::Set),
            SyntaxKind::SetMut(_) => Shape::Seq(SyntaxKind::SetMut),
            SyntaxKind::Bytes(_) => Shape::Seq(SyntaxKind::Bytes),
            SyntaxKind::BytesMut(_) => Shape::Seq(SyntaxKind::BytesMut),
            SyntaxKind::Quote(_) => Shape::Wrap(SyntaxKind::Quote),
            SyntaxKind::Quasiquote(_) => Shape::Wrap(SyntaxKind::Quasiquote),
            SyntaxKind::Unquote(_) => Shape::Wrap(SyntaxKind::Unquote),
            SyntaxKind::UnquoteSplicing(_) => Shape::Wrap(SyntaxKind::UnquoteSplicing),
            SyntaxKind::Splice(_) => Shape::Wrap(SyntaxKind::Splice),
            SyntaxKind::SyntaxLiteral(_) => Shape::Wrap(SyntaxKind::SyntaxLiteral),
            _ => Shape::Atom,
        }
    }

    /// This kind's children: the elements of a sequence, the one child of a
    /// wrapping kind, or nothing for an atom.
    pub fn children(&self) -> &[Syntax] {
        match self {
            SyntaxKind::List(items)
            | SyntaxKind::Array(items)
            | SyntaxKind::ArrayMut(items)
            | SyntaxKind::Struct(items)
            | SyntaxKind::StructMut(items)
            | SyntaxKind::Set(items)
            | SyntaxKind::SetMut(items)
            | SyntaxKind::Bytes(items)
            | SyntaxKind::BytesMut(items) => items.as_slice(),
            SyntaxKind::Quote(inner)
            | SyntaxKind::Quasiquote(inner)
            | SyntaxKind::Unquote(inner)
            | SyntaxKind::UnquoteSplicing(inner)
            | SyntaxKind::Splice(inner) => unsafe { std::slice::from_raw_parts(inner.as_ptr(), 1) },
            // A syntax literal's child is data the walkers must not descend
            // into: it carries the scopes of the context it was captured in.
            // Reporting no children is how `children`/`rebuild` state that.
            SyntaxKind::SyntaxLiteral(_) => &[],
            _ => &[],
        }
    }

    /// This kind again, over `items` — the inverse of [`children`](Self::children).
    ///
    /// An atom, and a `SyntaxLiteral` (which reports no children), rebuild as
    /// themselves. A wrapping kind takes `items[0]`.
    pub(crate) fn rebuild(&self, arena: &SyntaxArena, items: &[Syntax]) -> SyntaxKind {
        match self.shape() {
            Shape::Atom => *self,
            Shape::Seq(make) => make(arena.nodes(items)),
            Shape::Wrap(make) => make(arena.node(items[0])),
        }
    }
}

impl Syntax {
    /// Create a new Syntax node with empty scope set
    pub fn new(kind: SyntaxKind, span: Span) -> Self {
        Syntax {
            kind,
            span,
            scopes: RegionSlice::empty(),
            scope_exempt: false,
        }
    }

    /// Create a new Syntax node with the given scope set, copied into `arena`.
    pub(crate) fn with_scopes(
        arena: &SyntaxArena,
        kind: SyntaxKind,
        span: Span,
        scopes: &[ScopeId],
    ) -> Self {
        Syntax {
            kind,
            span,
            scopes: arena.scopes(scopes),
            scope_exempt: false,
        }
    }

    /// Create a new Syntax node sharing an existing scope slice.
    ///
    /// The slice must already live in the arena this node is built for — the
    /// usual case, where a rewrite keeps a node's scopes and changes only its
    /// kind. Sharing a slice from a *shorter-lived* arena is the mistake this
    /// spelling makes visible; [`with_scopes`](Self::with_scopes) copies.
    pub(crate) fn with_scope_slice(
        kind: SyntaxKind,
        span: Span,
        scopes: RegionSlice<ScopeId>,
    ) -> Self {
        Syntax {
            kind,
            span,
            scopes,
            scope_exempt: false,
        }
    }

    /// This node's scope set.
    pub(crate) fn scopes(&self) -> &[ScopeId] {
        self.scopes.as_slice()
    }

    /// This node's scope set as the region handle, for a rewrite that keeps
    /// the scopes and stays in the same arena.
    pub(crate) fn scope_slice(&self) -> RegionSlice<ScopeId> {
        self.scopes
    }

    /// Add a scope to this node's scope set
    pub(crate) fn add_scope(&mut self, arena: &SyntaxArena, scope: ScopeId) {
        if self.scopes.contains(&scope) {
            return;
        }
        let mut next: Vec<ScopeId> = self.scopes.as_slice().to_vec();
        next.push(scope);
        self.scopes = arena.scopes(&next);
    }

    /// Flip `scope` on this node: remove it if present, add it if absent.
    /// The macro-expansion hygiene operation (Flatt's sets-of-scopes):
    /// applied to everything a transformer returns, it MARKS
    /// template-origin identifiers (which never saw the intro scope) and
    /// UNMARKS argument-origin identifiers (pre-stamped before the call),
    /// distinguishing the two without tracking provenance.
    pub(crate) fn flip_scope(&mut self, arena: &SyntaxArena, scope: ScopeId) {
        let mut next: Vec<ScopeId> = self.scopes.as_slice().to_vec();
        match next.iter().position(|s| *s == scope) {
            Some(pos) => {
                next.remove(pos);
            }
            None => next.push(scope),
        }
        self.scopes = arena.scopes(&next);
    }

    /// Replace this node's scope set outright.
    pub(crate) fn set_scopes(&mut self, arena: &SyntaxArena, scopes: &[ScopeId]) {
        self.scopes = arena.scopes(scopes);
    }

    /// Give this node and every descendant `scopes`, and mark them all
    /// scope-exempt.
    ///
    /// `datum->syntax` uses this to give a datum the lexical context of
    /// another syntax object while stopping `add_scope_recursive` from
    /// overriding those scopes. It writes through the tree, so the caller must
    /// own it uniquely — see [`children_mut`](Self::children_mut).
    pub(crate) fn set_scopes_recursive(&mut self, arena: &SyntaxArena, scopes: &[ScopeId]) {
        self.set_scopes(arena, scopes);
        self.scope_exempt = true;
        for child in self.children_mut() {
            child.set_scopes_recursive(arena, scopes);
        }
    }

    /// The children of this node, as a slice this node's holder may mutate.
    ///
    /// **The caller must own the tree uniquely.** A `Syntax` is `Copy` and
    /// subtrees are shared by pointer, so writing through this handle is
    /// visible to every other holder of the same child slice. Legal where the
    /// caller built the subtree and no one else has it — the expander's
    /// in-place walks over a freshly stamped copy. Everywhere else, build a
    /// copy with [`copy_into`](Self::copy_into) and mutate that.
    pub(crate) fn children_mut(&mut self) -> &mut [Syntax] {
        let children = self.kind.children();
        let (ptr, len) = (children.as_ptr(), children.len());
        // The region owns these bytes and this node names them; `children`
        // hands out a shared view of the same memory, which the unique
        // ownership the doc comment demands makes safe to widen.
        unsafe { std::slice::from_raw_parts_mut(ptr as *mut Syntax, len) }
    }

    /// Deep-copy this tree into `arena`.
    ///
    /// Every node, child slice, string payload, and scope set is rebuilt in
    /// the destination, so the copy shares nothing with the source and
    /// outlives it. This is what lets a tree cross an arena boundary: out of
    /// a macro transformer's scratch into the working arena, out of the
    /// working arena into the template arena, and out of either into a
    /// `Value`'s own region.
    pub fn copy_into(&self, arena: &SyntaxArena) -> Syntax {
        let kind = match &self.kind {
            SyntaxKind::Symbol(s) => SyntaxKind::Symbol(arena.text(s)),
            SyntaxKind::Keyword(s) => SyntaxKind::Keyword(arena.text(s)),
            SyntaxKind::String(s) => SyntaxKind::String(arena.text(s)),
            SyntaxKind::StringMut(s) => SyntaxKind::StringMut(arena.text(s)),
            SyntaxKind::SyntaxLiteral(inner) => {
                SyntaxKind::SyntaxLiteral(arena.node(inner.copy_into(arena)))
            }
            other => {
                let kids: Vec<Syntax> = other
                    .children()
                    .iter()
                    .map(|c| c.copy_into(arena))
                    .collect();
                other.rebuild(arena, &kids)
            }
        };
        Syntax {
            kind,
            span: self.span,
            scopes: arena.scopes(self.scopes.as_slice()),
            scope_exempt: self.scope_exempt,
        }
    }

    /// Check if this is a symbol with the given name
    pub fn is_symbol(&self, name: &str) -> bool {
        matches!(&self.kind, SyntaxKind::Symbol(s) if s.as_str() == name)
    }

    /// Get symbol name if this is a symbol
    pub fn as_symbol(&self) -> Option<&str> {
        match &self.kind {
            SyntaxKind::Symbol(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Get list contents if this is a list
    pub fn as_list(&self) -> Option<&[Syntax]> {
        match &self.kind {
            SyntaxKind::List(items) => Some(items.as_slice()),
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
            SyntaxKind::List(items) | SyntaxKind::Array(items) => Some(items.as_slice()),
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

// ── constructors ─────────────────────────────────────────────────────────
//
// The payload of a name, a literal, or a compound is region data, so building
// one takes an arena. These are the funnels; a caller that writes
// `SyntaxKind::Symbol(...)` by hand would have to reach for `arena.text`
// itself, which is what these exist to prevent.

impl Syntax {
    /// A symbol node named `name`.
    pub fn symbol(arena: &SyntaxArena, name: &str, span: Span) -> Syntax {
        Syntax::new(SyntaxKind::Symbol(arena.text(name)), span)
    }

    /// A symbol node named `name`, carrying `scopes`.
    pub(crate) fn symbol_scoped(
        arena: &SyntaxArena,
        name: &str,
        span: Span,
        scopes: &[ScopeId],
    ) -> Syntax {
        Syntax::with_scopes(arena, SyntaxKind::Symbol(arena.text(name)), span, scopes)
    }

    /// A keyword node named `name`.
    pub fn keyword(arena: &SyntaxArena, name: &str, span: Span) -> Syntax {
        Syntax::new(SyntaxKind::Keyword(arena.text(name)), span)
    }

    /// An immutable string literal node.
    pub fn string(arena: &SyntaxArena, s: &str, span: Span) -> Syntax {
        Syntax::new(SyntaxKind::String(arena.text(s)), span)
    }

    /// A mutable string literal node (`@"..."`).
    pub fn string_mut(arena: &SyntaxArena, s: &str, span: Span) -> Syntax {
        Syntax::new(SyntaxKind::StringMut(arena.text(s)), span)
    }

    /// A list node over `items`.
    pub fn list(arena: &SyntaxArena, items: &[Syntax], span: Span) -> Syntax {
        Syntax::new(SyntaxKind::List(arena.nodes(items)), span)
    }

    /// An immutable array node over `items`.
    pub fn array(arena: &SyntaxArena, items: &[Syntax], span: Span) -> Syntax {
        Syntax::new(SyntaxKind::Array(arena.nodes(items)), span)
    }

    /// A quote node over `inner`.
    pub fn quote(arena: &SyntaxArena, inner: Syntax, span: Span) -> Syntax {
        Syntax::new(SyntaxKind::Quote(arena.node(inner)), span)
    }
}
