// audited: 2026-10-22
//! Expand the elements of a list, array, struct or set literal, and splice an include among them.
//!
//! docs/include.md

use super::*;

impl Expander {
    /// Expand every element of a sequence literal and rebuild it under `kind`.
    /// Shared by the list/array/struct/set (and mutable) literal arms of
    /// `expand_into`; `kind` is the tuple-variant constructor for the rebuilt
    /// node. An element that is an include becomes the forms of its file.
    pub(super) fn expand_seq(
        &mut self,
        items: &[Syntax],
        node: &Syntax,
        symbols: &mut SymbolTable,
        vm: &mut VM,
        kind: crate::syntax::SeqCtor,
    ) -> Result<Syntax, String> {
        let mut expanded = Vec::with_capacity(items.len());
        for item in items {
            self.expand_into(*item, &mut expanded, symbols, vm)?;
        }
        // The rebuilt node keeps `node`'s scope slice rather than copying it:
        // both live in this expansion's working arena.
        Ok(Syntax::with_scope_slice(
            kind(self.arena().nodes(&expanded)),
            node.span,
            node.scope_slice(),
        ))
    }
}
