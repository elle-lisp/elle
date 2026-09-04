//! Quasiquote expansion to runtime list construction

use super::Expander;
use crate::symbol::SymbolTable;
use crate::syntax::{Span, Syntax, SyntaxKind};
use crate::vm::VM;

impl Expander {
    /// Convert quasiquote to code that constructs the value at runtime
    /// depth tracks nesting level for nested quasiquotes
    pub(super) fn quasiquote_to_code(
        &mut self,
        syntax: &Syntax,
        depth: usize,
        span: &Span,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        match &syntax.kind {
            // Unquote at depth 1 - evaluate the expression
            SyntaxKind::Unquote(inner) if depth == 1 => self.expand(**inner, symbols, vm),

            // Nested unquote - decrease depth
            SyntaxKind::Unquote(inner) if depth > 1 => {
                let expanded = self.quasiquote_to_code(inner, depth - 1, span, symbols, vm)?;
                // Wrap in (list (quote unquote) expanded)
                Ok(self.make_list(
                    vec![
                        self.make_symbol("list", *span),
                        self.make_list(
                            vec![
                                self.make_symbol("quote", *span),
                                self.make_symbol("unquote", *span),
                            ],
                            *span,
                        ),
                        expanded,
                    ],
                    *span,
                ))
            }

            // Nested quasiquote - increase depth
            SyntaxKind::Quasiquote(inner) => {
                let expanded = self.quasiquote_to_code(inner, depth + 1, span, symbols, vm)?;
                Ok(self.make_list(
                    vec![
                        self.make_symbol("list", *span),
                        self.make_list(
                            vec![
                                self.make_symbol("quote", *span),
                                self.make_symbol("quasiquote", *span),
                            ],
                            *span,
                        ),
                        expanded,
                    ],
                    *span,
                ))
            }

            // List - process elements, handling unquote-splicing
            SyntaxKind::List(items) => {
                self.quasiquote_list_to_code(items, depth, span, symbols, vm)
            }

            // Array (bracket syntax) — process elements so unquote/splice work
            // inside `[...]`, producing a runtime array via the `array` primitive.
            // This enables `[,a ,b]` in quasiquote and bracket bindings in
            // macro output: `(let [,name ,val] ...)`.
            SyntaxKind::Array(items) => {
                self.quasiquote_array_to_code(items, depth, span, symbols, vm)
            }

            // StructMut — quasiquote treats it as data (quoted)
            SyntaxKind::StructMut(_) => {
                Ok(self.make_list(vec![self.make_symbol("quote", *span), *syntax], *span))
            }

            // Symbols: wrap as SyntaxLiteral to preserve definition-site
            // scopes through the Value round-trip (Flatt 2016 §3). Without
            // this, template symbols become bare Values and lose their scopes.
            // The intro scope is added later by add_scope_recursive, which
            // together with the definition-site scopes ensures correct
            // resolution even when the call site shadows the same name.
            SyntaxKind::Symbol(_) => Ok(Syntax::new(
                // Carry the template symbol as plain compile-time data (its scope
                // set rides along in the `Syntax`), NOT a heap `Value`. The
                // analyzer materializes it as a fresh ordinary allocation per
                // execution.
                SyntaxKind::SyntaxLiteral(self.arena().node(*syntax)),
                *span,
            )),

            // Everything else gets quoted (atoms don't participate in binding)
            _ => Ok(self.make_list(vec![self.make_symbol("quote", *span), *syntax], *span)),
        }
    }

    /// Convert a quasiquoted array `[...]` to code that constructs an
    /// immutable array at runtime. Uses the `array` primitive so that
    /// unquote/splice work inside brackets in macro templates. The macro
    /// evaluator produces an immutable array Value, which `from_value`
    /// converts back to `SyntaxKind::Array` — preserving bracket syntax
    /// through the expansion round-trip.
    pub(super) fn quasiquote_array_to_code(
        &mut self,
        items: &[Syntax],
        depth: usize,
        span: &Span,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        let mut array_call = vec![self.make_symbol("array", *span)];
        for item in items {
            array_call.push(self.quasiquote_to_code(item, depth, span, symbols, vm)?);
        }
        Ok(self.make_list(array_call, *span))
    }

    /// Convert a quasiquoted list to code
    pub(super) fn quasiquote_list_to_code(
        &mut self,
        items: &[Syntax],
        depth: usize,
        span: &Span,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        if items.is_empty() {
            return Ok(self.make_list(
                vec![
                    self.make_symbol("quote", *span),
                    self.make_list(vec![], *span),
                ],
                *span,
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
                        let mut list_call = vec![self.make_symbol("list", *span)];
                        list_call.append(&mut current_segment);
                        segments.push(self.make_list(list_call, *span));
                    }
                    // Add spliced expression
                    if depth == 1 {
                        segments.push(self.expand(**inner, symbols, vm)?);
                    } else {
                        segments.push(self.quasiquote_to_code(
                            inner,
                            depth - 1,
                            span,
                            symbols,
                            vm,
                        )?);
                    }
                } else {
                    current_segment.push(self.quasiquote_to_code(item, depth, span, symbols, vm)?);
                }
            }

            // Flush remaining segment
            if !current_segment.is_empty() {
                let mut list_call = vec![self.make_symbol("list", *span)];
                list_call.extend(current_segment);
                segments.push(self.make_list(list_call, *span));
            }

            // Build nested binary append calls: (append seg1 (append seg2 (append seg3 ...)))
            // append is now binary, so we need to nest the calls
            let mut result = segments
                .pop()
                .unwrap_or(self.make_list(vec![self.make_symbol("list", *span)], *span));
            while let Some(seg) = segments.pop() {
                result =
                    self.make_list(vec![self.make_symbol("append", *span), seg, result], *span);
            }
            Ok(result)
        } else {
            // Simple case - just use list
            let mut list_call = vec![self.make_symbol("list", *span)];
            for item in items {
                list_call.push(self.quasiquote_to_code(item, depth, span, symbols, vm)?);
            }
            Ok(self.make_list(list_call, *span))
        }
    }
}
