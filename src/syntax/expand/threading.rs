//! Threading macro expansion (-> and ->>)

use super::Expander;
use crate::syntax::{Span, Syntax, SyntaxKind};

impl Expander {
    /// Handle thread-first macro: (-> value form1 form2 ...)
    /// Inserts value as the FIRST argument to each form
    pub(super) fn handle_thread_first(
        &mut self,
        items: &[Syntax],
        span: &Span,
    ) -> Result<Syntax, String> {
        if items.len() < 2 {
            return Err(format!("{}: -> requires at least a value", span));
        }

        // Start with the initial value
        let mut result = items[1].clone();

        // Thread through each form
        for form in &items[2..] {
            result = match &form.kind {
                SyntaxKind::List(form_items) if !form_items.is_empty() => {
                    // Insert result as first argument: (f a b) becomes (f result a b)
                    let mut new_items = vec![form_items[0].clone(), result];
                    new_items.extend(form_items[1..].iter().cloned());
                    Syntax::new(SyntaxKind::List(new_items), span.clone())
                }
                SyntaxKind::Symbol(_) => {
                    // Bare symbol: f becomes (f result)
                    Syntax::new(SyntaxKind::List(vec![form.clone(), result]), span.clone())
                }
                _ => {
                    return Err(format!("{}: -> form must be a list or symbol", span));
                }
            };
        }

        // Recursively expand the result
        self.expand(result)
    }

    /// Handle thread-last macro: (->> value form1 form2 ...)
    /// Inserts value as the LAST argument to each form
    pub(super) fn handle_thread_last(
        &mut self,
        items: &[Syntax],
        span: &Span,
    ) -> Result<Syntax, String> {
        if items.len() < 2 {
            return Err(format!("{}: ->> requires at least a value", span));
        }

        // Start with the initial value
        let mut result = items[1].clone();

        // Thread through each form
        for form in &items[2..] {
            result = match &form.kind {
                SyntaxKind::List(form_items) if !form_items.is_empty() => {
                    // Insert result as last argument: (f a b) becomes (f a b result)
                    let mut new_items = form_items.to_vec();
                    new_items.push(result);
                    Syntax::new(SyntaxKind::List(new_items), span.clone())
                }
                SyntaxKind::Symbol(_) => {
                    // Bare symbol: f becomes (f result)
                    Syntax::new(SyntaxKind::List(vec![form.clone(), result]), span.clone())
                }
                _ => {
                    return Err(format!("{}: ->> form must be a list or symbol", span));
                }
            };
        }

        // Recursively expand the result
        self.expand(result)
    }
}
