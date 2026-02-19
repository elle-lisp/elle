//! Macro call expansion and substitution

use super::{Expander, MacroDef, SyntaxKind};
use crate::syntax::Syntax;

impl Expander {
    pub(super) fn expand_macro_call(
        &mut self,
        macro_def: &MacroDef,
        args: &[Syntax],
        _call_site: &Syntax,
    ) -> Result<Syntax, String> {
        // Check arity
        if args.len() != macro_def.params.len() {
            return Err(format!(
                "Macro '{}' expects {} arguments, got {}",
                macro_def.name,
                macro_def.params.len(),
                args.len()
            ));
        }

        // Generate fresh scope for this macro expansion
        let intro_scope = self.fresh_scope();

        // Substitute parameters with arguments in template
        let substituted = self.substitute(&macro_def.template, &macro_def.params, args);

        // If the template was a quasiquote, evaluate it to produce Syntax directly
        // instead of converting to (list ...) calls
        let resolved = match &substituted.kind {
            SyntaxKind::Quasiquote(inner) => self.eval_quasiquote_to_syntax(inner)?,
            _ => substituted,
        };

        // Add intro_scope to all identifiers introduced by the macro
        let hygienized = self.add_scope_recursive(resolved, intro_scope);

        // Recursively expand the result
        self.expand(hygienized)
    }

    /// Evaluate a quasiquote at the Syntax level, producing a Syntax tree directly.
    /// This is used for macro templates where we want compile-time Syntax construction,
    /// not runtime list construction via (list ...) calls.
    ///
    /// At this point, parameters have already been substituted, so:
    /// - Unquote nodes contain the substituted argument Syntax
    /// - UnquoteSplicing nodes contain the substituted argument Syntax (should be a list)
    /// - Everything else is literal and should be kept as-is
    pub(super) fn eval_quasiquote_to_syntax(&self, syntax: &Syntax) -> Result<Syntax, String> {
        match &syntax.kind {
            SyntaxKind::Unquote(inner) => {
                // The unquote content has already been substituted with the argument
                // Just unwrap and return the substituted Syntax
                Ok((**inner).clone())
            }
            SyntaxKind::List(items) => {
                let mut result = Vec::new();
                for item in items {
                    match &item.kind {
                        SyntaxKind::UnquoteSplicing(inner) => {
                            // Splice: the inner should be a list, add its elements
                            if let SyntaxKind::List(splice_items) = &inner.kind {
                                // Recursively evaluate each spliced item
                                for splice_item in splice_items {
                                    result.push(self.eval_quasiquote_to_syntax(splice_item)?);
                                }
                            } else {
                                // If it's not a list, just add the single item
                                result.push((**inner).clone());
                            }
                        }
                        _ => {
                            result.push(self.eval_quasiquote_to_syntax(item)?);
                        }
                    }
                }
                Ok(Syntax::with_scopes(
                    SyntaxKind::List(result),
                    syntax.span.clone(),
                    syntax.scopes.clone(),
                ))
            }
            SyntaxKind::Vector(items) => {
                let mut result = Vec::new();
                for item in items {
                    match &item.kind {
                        SyntaxKind::UnquoteSplicing(inner) => {
                            if let SyntaxKind::List(splice_items) = &inner.kind {
                                for splice_item in splice_items {
                                    result.push(self.eval_quasiquote_to_syntax(splice_item)?);
                                }
                            } else {
                                result.push((**inner).clone());
                            }
                        }
                        _ => {
                            result.push(self.eval_quasiquote_to_syntax(item)?);
                        }
                    }
                }
                Ok(Syntax::with_scopes(
                    SyntaxKind::Vector(result),
                    syntax.span.clone(),
                    syntax.scopes.clone(),
                ))
            }
            // Nested quasiquote - keep as-is
            // This handles cases like ``(a ,b) where we have nested quasiquotes
            SyntaxKind::Quasiquote(_) => {
                // For nested quasiquotes in macro templates, we keep the quasiquote
                // structure - it will be evaluated later
                Ok(syntax.clone())
            }
            // Anything else (symbols, ints, etc.) is literal - keep as-is
            _ => Ok(syntax.clone()),
        }
    }

    pub(super) fn substitute(
        &self,
        template: &Syntax,
        params: &[String],
        args: &[Syntax],
    ) -> Syntax {
        match &template.kind {
            SyntaxKind::Symbol(name) => {
                // If this symbol is a parameter, substitute with argument
                if let Some(idx) = params.iter().position(|p| p == name) {
                    args[idx].clone()
                } else {
                    template.clone()
                }
            }
            SyntaxKind::List(items) => {
                let new_items: Vec<Syntax> = items
                    .iter()
                    .map(|item| self.substitute(item, params, args))
                    .collect();
                Syntax::with_scopes(
                    SyntaxKind::List(new_items),
                    template.span.clone(),
                    template.scopes.clone(),
                )
            }
            SyntaxKind::Vector(items) => {
                let new_items: Vec<Syntax> = items
                    .iter()
                    .map(|item| self.substitute(item, params, args))
                    .collect();
                Syntax::with_scopes(
                    SyntaxKind::Vector(new_items),
                    template.span.clone(),
                    template.scopes.clone(),
                )
            }
            SyntaxKind::Quote(_) => {
                // Don't substitute inside quote
                template.clone()
            }
            SyntaxKind::Quasiquote(inner) => {
                let new_inner = self.substitute_quasiquote(inner, params, args);
                Syntax::with_scopes(
                    SyntaxKind::Quasiquote(Box::new(new_inner)),
                    template.span.clone(),
                    template.scopes.clone(),
                )
            }
            // Handle Unquote directly in templates (templates are implicitly quasiquoted)
            SyntaxKind::Unquote(inner) => {
                // Substitute inside the unquote and unwrap
                self.substitute(inner, params, args)
            }
            SyntaxKind::UnquoteSplicing(inner) => {
                // Substitute inside - splicing handled elsewhere
                let substituted = self.substitute(inner, params, args);
                Syntax::with_scopes(
                    SyntaxKind::UnquoteSplicing(Box::new(substituted)),
                    template.span.clone(),
                    template.scopes.clone(),
                )
            }
            _ => template.clone(),
        }
    }

    pub(super) fn substitute_quasiquote(
        &self,
        template: &Syntax,
        params: &[String],
        args: &[Syntax],
    ) -> Syntax {
        match &template.kind {
            SyntaxKind::Unquote(inner) => {
                // Inside unquote, do substitute
                let substituted = self.substitute(inner, params, args);
                Syntax::with_scopes(
                    SyntaxKind::Unquote(Box::new(substituted)),
                    template.span.clone(),
                    template.scopes.clone(),
                )
            }
            SyntaxKind::UnquoteSplicing(inner) => {
                let substituted = self.substitute(inner, params, args);
                Syntax::with_scopes(
                    SyntaxKind::UnquoteSplicing(Box::new(substituted)),
                    template.span.clone(),
                    template.scopes.clone(),
                )
            }
            SyntaxKind::List(items) => {
                let new_items: Vec<Syntax> = items
                    .iter()
                    .map(|item| self.substitute_quasiquote(item, params, args))
                    .collect();
                Syntax::with_scopes(
                    SyntaxKind::List(new_items),
                    template.span.clone(),
                    template.scopes.clone(),
                )
            }
            _ => template.clone(),
        }
    }
}
