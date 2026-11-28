// audited: 2026-11-24
//! Quasiquote lowering: a template becomes the code that builds its value.
//!
//! docs/syntax.md
//! docs/macros.md
//! src/syntax/expand/AGENTS.md

use super::Expander;
use crate::symbol::SymbolTable;
use crate::syntax::{Span, Syntax, SyntaxKind};
use crate::vm::VM;

impl Expander {
    /// The code that builds the value of the template `syntax` at nesting
    /// level `depth`, which is one inside the outermost quasiquote.
    pub(super) fn quasiquote_to_code(
        &mut self,
        syntax: &Syntax,
        depth: usize,
        span: &Span,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        match &syntax.kind {
            // An unquote that reaches level zero is the expression itself.
            SyntaxKind::Unquote(inner) if depth == 1 => self.expand(**inner, symbols, vm),
            SyntaxKind::Unquote(inner) => {
                self.level_form("unquote", inner, depth - 1, span, symbols, vm)
            }
            SyntaxKind::Quasiquote(inner) => {
                self.level_form("quasiquote", inner, depth + 1, span, symbols, vm)
            }
            // A quote keeps the level, so the holes inside it evaluate.
            SyntaxKind::Quote(inner) => self.level_form("quote", inner, depth, span, symbols, vm),
            SyntaxKind::List(items) if items.is_empty() => Ok(self.quoted(*syntax, span)),
            SyntaxKind::List(items) => {
                self.constructor_call("list", items, depth, span, symbols, vm)
            }
            SyntaxKind::Array(items) => {
                self.constructor_call("array", items, depth, span, symbols, vm)
            }
            // Code that runs at expansion builds code, so a template symbol
            // there keeps the scopes of the place it was written. The analyzer
            // materializes the literal as a syntax object.
            SyntaxKind::Symbol(_) if self.in_transformer => Ok(Syntax::new(
                SyntaxKind::SyntaxLiteral(self.arena().node(*syntax)),
                *span,
            )),
            // Every other form is data: an atom, a struct, or a symbol in code
            // that runs after expansion.
            _ => Ok(self.quoted(*syntax, span)),
        }
    }

    /// `(list 'name level)`: a quoting form inside the template, which stays a
    /// form in the value.
    fn level_form(
        &mut self,
        name: &str,
        inner: &Syntax,
        depth: usize,
        span: &Span,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        let level = self.quasiquote_to_code(inner, depth, span, symbols, vm)?;
        let head = self.quoted(self.make_symbol(name, *span), span);
        Ok(self.make_list(vec![self.make_symbol("list", *span), head, level], *span))
    }

    /// `(quote form)`.
    fn quoted(&self, form: Syntax, span: &Span) -> Syntax {
        Syntax::quote(&self.arena(), form, *span)
    }

    /// What an unquote-splice of `value` spreads. In code that runs at
    /// expansion the value is often a syntax object, a macro's argument, so
    /// there a syntax object spreads its children:
    /// `(let [spread value] (if (%eq (type-of spread) :syntax) (syntax/children spread) spread))`.
    /// `value` sits in the binding's init, so the name cannot capture it.
    fn spread_of(&self, value: Syntax, span: &Span) -> Syntax {
        if !self.in_transformer {
            return value;
        }
        let s = *span;
        let sym = |name| self.make_symbol(name, s);
        let is_syntax = self.make_list(
            vec![
                sym("%eq"),
                self.make_list(vec![sym("type-of"), sym("spread")], s),
                Syntax::keyword(&self.arena(), "syntax", s),
            ],
            s,
        );
        let children = self.make_list(vec![sym("syntax/children"), sym("spread")], s);
        let choice = self.make_list(vec![sym("if"), is_syntax, children, sym("spread")], s);
        let binding = Syntax::array(&self.arena(), &[sym("spread"), value], s);
        self.make_list(vec![sym("let"), binding, choice], s)
    }

    /// `(constructor item …)` over a list or array template's items. An
    /// unquote-splice at level one spreads its value into the call, which
    /// takes a list, an array or an `@array`.
    fn constructor_call(
        &mut self,
        constructor: &str,
        items: &[Syntax],
        depth: usize,
        span: &Span,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        let mut call = vec![self.make_symbol(constructor, *span)];
        for item in items {
            call.push(match &item.kind {
                SyntaxKind::UnquoteSplicing(inner) if depth == 1 => {
                    let value = self.expand(**inner, symbols, vm)?;
                    let spread = self.spread_of(value, span);
                    Syntax::new(SyntaxKind::Splice(self.arena().node(spread)), *span)
                }
                SyntaxKind::UnquoteSplicing(inner) => {
                    self.level_form("unquote-splicing", inner, depth - 1, span, symbols, vm)?
                }
                _ => self.quasiquote_to_code(item, depth, span, symbols, vm)?,
            });
        }
        Ok(self.make_list(call, *span))
    }
}
