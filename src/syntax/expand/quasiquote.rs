//! Quasiquote expansion to runtime list construction

use super::Expander;
use crate::syntax::{Span, Syntax, SyntaxKind};

impl Expander {
    /// Convert quasiquote to code that constructs the value at runtime
    /// depth tracks nesting level for nested quasiquotes
    pub(super) fn quasiquote_to_code(
        &mut self,
        syntax: &Syntax,
        depth: usize,
        span: &Span,
    ) -> Result<Syntax, String> {
        match &syntax.kind {
            // Unquote at depth 1 - evaluate the expression
            SyntaxKind::Unquote(inner) if depth == 1 => self.expand((**inner).clone()),

            // Nested unquote - decrease depth
            SyntaxKind::Unquote(inner) if depth > 1 => {
                let expanded = self.quasiquote_to_code(inner, depth - 1, span)?;
                // Wrap in (list (quote unquote) expanded)
                Ok(self.make_list(
                    vec![
                        self.make_symbol("list", span.clone()),
                        self.make_list(
                            vec![
                                self.make_symbol("quote", span.clone()),
                                self.make_symbol("unquote", span.clone()),
                            ],
                            span.clone(),
                        ),
                        expanded,
                    ],
                    span.clone(),
                ))
            }

            // Nested quasiquote - increase depth
            SyntaxKind::Quasiquote(inner) => {
                let expanded = self.quasiquote_to_code(inner, depth + 1, span)?;
                Ok(self.make_list(
                    vec![
                        self.make_symbol("list", span.clone()),
                        self.make_list(
                            vec![
                                self.make_symbol("quote", span.clone()),
                                self.make_symbol("quasiquote", span.clone()),
                            ],
                            span.clone(),
                        ),
                        expanded,
                    ],
                    span.clone(),
                ))
            }

            // List - process elements, handling unquote-splicing
            SyntaxKind::List(items) => self.quasiquote_list_to_code(items, depth, span),

            // Everything else gets quoted
            _ => Ok(self.make_list(
                vec![self.make_symbol("quote", span.clone()), syntax.clone()],
                span.clone(),
            )),
        }
    }

    /// Convert a quasiquoted list to code
    pub(super) fn quasiquote_list_to_code(
        &mut self,
        items: &[Syntax],
        depth: usize,
        span: &Span,
    ) -> Result<Syntax, String> {
        if items.is_empty() {
            return Ok(self.make_list(
                vec![
                    self.make_symbol("quote", span.clone()),
                    self.make_list(vec![], span.clone()),
                ],
                span.clone(),
            ));
        }

        // Check if any element is unquote-splicing
        let has_splice = items
            .iter()
            .any(|item| matches!(item.kind, SyntaxKind::UnquoteSplicing(_)));

        if has_splice {
            // Need to use append for splicing
            let mut segments = Vec::new();
            let mut current_segment = Vec::new();

            for item in items {
                if let SyntaxKind::UnquoteSplicing(inner) = &item.kind {
                    // Flush current segment
                    if !current_segment.is_empty() {
                        let mut list_call = vec![self.make_symbol("list", span.clone())];
                        list_call.append(&mut current_segment);
                        segments.push(self.make_list(list_call, span.clone()));
                    }
                    // Add spliced expression
                    if depth == 1 {
                        segments.push(self.expand((**inner).clone())?);
                    } else {
                        segments.push(self.quasiquote_to_code(inner, depth - 1, span)?);
                    }
                } else {
                    current_segment.push(self.quasiquote_to_code(item, depth, span)?);
                }
            }

            // Flush remaining segment
            if !current_segment.is_empty() {
                let mut list_call = vec![self.make_symbol("list", span.clone())];
                list_call.extend(current_segment);
                segments.push(self.make_list(list_call, span.clone()));
            }

            // Build (append seg1 seg2 ...)
            let mut append_call = vec![self.make_symbol("append", span.clone())];
            append_call.extend(segments);
            Ok(self.make_list(append_call, span.clone()))
        } else {
            // Simple case - just use list
            let mut list_call = vec![self.make_symbol("list", span.clone())];
            for item in items {
                list_call.push(self.quasiquote_to_code(item, depth, span)?);
            }
            Ok(self.make_list(list_call, span.clone()))
        }
    }
}
