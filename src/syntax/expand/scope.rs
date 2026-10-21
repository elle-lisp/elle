// audited: 2026-10-21
//! The scopes an expander mints for its unit, and the walks that stamp and flip them.
//!
//! docs/macros.md
//! docs/impl/syntax.md

use super::*;

/// Scope operation applied by `map_scope_recursive`. Both operations honor
/// `scope_exempt`: a datum->syntax node keeps exactly the scopes its context
/// gave it. The context's scopes at transformer time include the expansion's
/// pre-stamped intro scope, which datum->syntax itself strips (see
/// `prim_datum_to_syntax`), so an exempt node already carries its TRUE
/// use-site scope set and must dodge the flip.
#[derive(Clone, Copy)]
enum ScopeOp {
    Add,
    Flip,
}

impl Expander {
    /// Mint the unit's next ordinary scope.
    pub(crate) fn fresh_scope(&mut self) -> ScopeId {
        ScopeId::new(self.unit.tag(), self.unit.next_scope())
    }

    /// Mint the unit's next INTRO scope, from the same count with the intro
    /// bit set. One is minted per macro expansion and flipped onto
    /// template-origin nodes; the analyzer's referential-transparency rule
    /// keys on the bit.
    pub(crate) fn fresh_intro_scope(&mut self) -> ScopeId {
        ScopeId::intro(self.unit.tag(), self.unit.next_scope())
    }

    /// Stamp a fresh file scope onto a syntax tree. This distinguishes
    /// user bindings from primitives (which have empty scopes), enabling
    /// macro hygiene: template symbols from the prelude carry
    /// `ScopeId::PRIMITIVE` and won't match user bindings that carry the file
    /// scope instead.
    pub fn stamp_file_scope(&mut self, syntax: Syntax) -> Syntax {
        let scope = self.fresh_scope();
        self.add_scope_recursive(syntax, scope)
    }

    /// Public wrapper for adding a scope to a syntax tree.
    pub(crate) fn stamp_scope(&self, syntax: Syntax, scope: ScopeId) -> Syntax {
        self.add_scope_recursive(syntax, scope)
    }

    pub(super) fn add_scope_recursive(&self, syntax: Syntax, scope: ScopeId) -> Syntax {
        self.map_scope_recursive(syntax, scope, ScopeOp::Add)
    }

    /// Flip `scope` on every non-exempt node (see `Syntax::flip_scope`):
    /// the post-expansion hygiene operation. Template-origin identifiers
    /// gain the intro scope; argument-origin identifiers (pre-stamped at
    /// wrap time) lose it again. datum->syntax results are exempt: they
    /// already carry their context's true scopes, because
    /// `prim_datum_to_syntax` strips the intro scope at copy time.
    pub(super) fn flip_scope_recursive(&self, syntax: Syntax, scope: ScopeId) -> Syntax {
        self.map_scope_recursive(syntax, scope, ScopeOp::Flip)
    }

    /// Stamp `scope` onto `syntax` and, where it applies, onto its children.
    ///
    /// The walk COPIES: it builds a new node and a new child slice at every
    /// level it touches. Subtrees are shared by pointer, so a walk that wrote
    /// through the source would stamp trees the caller still holds — a macro
    /// argument's identifiers would gain a scope they never entered.
    fn map_scope_recursive(&self, syntax: Syntax, scope: ScopeId, op: ScopeOp) -> Syntax {
        // datum->syntax nodes keep their exact scopes — neither ordinary
        // stamping nor the hygiene flip touches them.
        if syntax.scope_exempt {
            return syntax;
        }

        let mut out = syntax;
        match op {
            ScopeOp::Add => out.add_scope(&self.arena, scope),
            ScopeOp::Flip => out.flip_scope(&self.arena, scope),
        }

        // A quote's contents are literal data, and a syntax literal already
        // carries the scopes of the context it was captured in. Neither
        // descends. (`SyntaxKind::children` reports no children for a syntax
        // literal, so only quote needs saying here.)
        if matches!(out.kind, SyntaxKind::Quote(_)) {
            return out;
        }

        let kind = out.kind;
        let children = kind.children();
        if children.is_empty() {
            return out;
        }
        let mapped: Vec<Syntax> = children
            .iter()
            .map(|item| self.map_scope_recursive(*item, scope, op))
            .collect();
        out.kind = kind.rebuild(&self.arena, &mapped);
        out
    }
}
