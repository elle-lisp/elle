use super::*;

impl Expander {
    /// Expand every element of a sequence literal and rebuild it under `kind`.
    /// Shared by the list/array/struct/set (and mutable) literal arms of
    /// `expand`; `kind` is the tuple-variant constructor for the rebuilt node.
    pub(super) fn expand_seq(
        &mut self,
        items: &[Syntax],
        span: Span,
        scopes: Vec<ScopeId>,
        symbols: &mut SymbolTable,
        vm: &mut VM,
        kind: fn(Vec<Syntax>) -> SyntaxKind,
    ) -> Result<Syntax, String> {
        let expanded: Result<Vec<Syntax>, String> = items
            .iter()
            .map(|item| self.expand(item.clone(), symbols, vm))
            .collect();
        Ok(Syntax::with_scopes(kind(expanded?), span, scopes))
    }
}
