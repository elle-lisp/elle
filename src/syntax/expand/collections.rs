use super::*;

impl Expander {
    /// Expand every element of a sequence literal and rebuild it under `kind`.
    /// Shared by the list/array/struct/set (and mutable) literal arms of
    /// `expand`; `kind` is the tuple-variant constructor for the rebuilt node.
    pub(super) fn expand_seq(
        &mut self,
        items: &[Syntax],
        node: &Syntax,
        symbols: &mut SymbolTable,
        vm: &mut VM,
        kind: crate::syntax::SeqCtor,
    ) -> Result<Syntax, String> {
        let expanded: Result<Vec<Syntax>, String> = items
            .iter()
            .map(|item| self.expand(*item, symbols, vm))
            .collect();
        // The rebuilt node keeps `node`'s scope slice rather than copying it:
        // both live in this expansion's working arena.
        Ok(Syntax::with_scope_slice(
            kind(self.arena().nodes(&expanded?)),
            node.span,
            node.scope_slice(),
        ))
    }
}
