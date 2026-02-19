//! Macro introspection: macro? and expand-macro

use super::Expander;
use crate::syntax::{Span, Syntax, SyntaxKind};

impl Expander {
    /// Handle (macro? symbol) - returns #t if symbol is a defined macro, #f otherwise
    ///
    /// This is handled at expansion time because:
    /// 1. The Expander knows which macros are defined
    /// 2. The symbol would otherwise be resolved as a variable by the analyzer
    pub(super) fn handle_macro_predicate(
        &self,
        items: &[Syntax],
        span: &Span,
    ) -> Result<Syntax, String> {
        // Syntax: (macro? symbol)
        if items.len() != 2 {
            return Err(format!(
                "{}: macro? requires exactly 1 argument, got {}",
                span,
                items.len() - 1
            ));
        }

        // The argument should be a symbol (not quoted - we check the raw symbol name)
        let is_macro = if let Some(name) = items[1].as_symbol() {
            self.macros.contains_key(name)
        } else {
            // Not a symbol - return false
            false
        };

        Ok(Syntax::new(SyntaxKind::Bool(is_macro), span.clone()))
    }

    /// Handle (expand-macro '(macro-call ...)) - returns the expanded form as data
    ///
    /// This expands the quoted form and wraps the result in a quote so it becomes
    /// data at runtime rather than being executed.
    pub(super) fn handle_expand_macro(
        &mut self,
        items: &[Syntax],
        span: &Span,
    ) -> Result<Syntax, String> {
        // Syntax: (expand-macro '(form ...))
        if items.len() != 2 {
            return Err(format!(
                "{}: expand-macro requires exactly 1 argument, got {}",
                span,
                items.len() - 1
            ));
        }

        // The argument should be a quoted form
        let form = match &items[1].kind {
            SyntaxKind::Quote(inner) => (**inner).clone(),
            _ => {
                // Not a quoted form - just return the argument unchanged
                // (This allows expand-macro to be a no-op for non-quoted args)
                return Ok(items[1].clone());
            }
        };

        // Expand the form (this will trigger macro expansion if it's a macro call)
        let expanded = self.expand(form)?;

        // Wrap the result in a quote so it becomes data at runtime
        Ok(Syntax::new(
            SyntaxKind::Quote(Box::new(expanded)),
            span.clone(),
        ))
    }
}
