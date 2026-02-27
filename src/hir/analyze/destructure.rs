//! Destructuring: pattern analysis and helpers for binding forms.

use super::*;
use crate::hir::pattern::HirPattern;
use crate::syntax::{ScopeId, Syntax, SyntaxKind};

impl<'a> Analyzer<'a> {
    /// Check if an expression is a var or def form and return all names being defined.
    /// For simple defines like `(def x ...)`, returns one name.
    /// For destructuring like `(def (a b) ...)`, returns all leaf names.
    pub(crate) fn is_define_form(syntax: &Syntax) -> Vec<(&str, &[ScopeId])> {
        if let SyntaxKind::List(items) = &syntax.kind {
            if let Some(first) = items.first() {
                if let Some(name) = first.as_symbol() {
                    if name == "var" || name == "def" {
                        if let Some(second) = items.get(1) {
                            // Simple symbol binding
                            if let Some(sym) = second.as_symbol() {
                                return vec![(sym, second.scopes.as_slice())];
                            }
                            // Destructuring pattern — extract all leaf symbols
                            let mut names = Vec::new();
                            Self::extract_pattern_names(second, &mut names);
                            return names;
                        }
                    }
                }
            }
        }
        Vec::new()
    }

    /// Recursively extract all symbol names from a syntax pattern (list, tuple, array, struct, or table).
    fn extract_pattern_names<'s>(syntax: &'s Syntax, out: &mut Vec<(&'s str, &'s [ScopeId])>) {
        match &syntax.kind {
            SyntaxKind::Symbol(name) if name == "_" || name == "&" => {
                // Skip wildcard and rest marker
            }
            SyntaxKind::Symbol(name) => {
                out.push((name.as_str(), syntax.scopes.as_slice()));
            }
            SyntaxKind::List(items) | SyntaxKind::Tuple(items) | SyntaxKind::Array(items) => {
                for item in items {
                    Self::extract_pattern_names(item, out);
                }
            }
            SyntaxKind::Struct(items) | SyntaxKind::Table(items) => {
                // Struct/table patterns are alternating keyword/pattern pairs;
                // only extract names from the value patterns (odd indices)
                for item in items.iter().skip(1).step_by(2) {
                    Self::extract_pattern_names(item, out);
                }
            }
            _ => {} // Ignore non-symbol, non-compound elements (including keywords)
        }
    }

    /// Check if a syntax node is a destructuring pattern (list, tuple, array, struct, or table).
    pub(super) fn is_destructure_pattern(syntax: &Syntax) -> bool {
        matches!(
            &syntax.kind,
            SyntaxKind::List(_)
                | SyntaxKind::Tuple(_)
                | SyntaxKind::Array(_)
                | SyntaxKind::Struct(_)
                | SyntaxKind::Table(_)
        )
    }

    /// Estimate arity from syntax-level parameter list (before analysis).
    /// Detects `&` for variadic functions.
    pub(super) fn arity_from_syntax_params(params: &[Syntax]) -> Arity {
        let has_rest = params
            .iter()
            .any(|s| matches!(&s.kind, SyntaxKind::Symbol(n) if n == "&"));
        if has_rest {
            // Count fixed params (everything before &)
            let fixed = params
                .iter()
                .position(|s| matches!(&s.kind, SyntaxKind::Symbol(n) if n == "&"))
                .unwrap_or(params.len());
            Arity::AtLeast(fixed)
        } else {
            Arity::Exact(params.len())
        }
    }

    /// Split a pattern's items at `&` into (fixed_elements, optional_rest).
    /// Validates: at most one `&`, exactly one pattern after `&`, `&` not at end.
    pub(super) fn split_rest_pattern<'s>(
        items: &'s [Syntax],
        span: &Span,
    ) -> Result<(&'s [Syntax], Option<&'s Syntax>), String> {
        let amp_pos = items
            .iter()
            .position(|s| matches!(&s.kind, SyntaxKind::Symbol(n) if n == "&"));
        match amp_pos {
            None => Ok((items, None)),
            Some(pos) => {
                // Check for multiple &
                let second = items[pos + 1..]
                    .iter()
                    .any(|s| matches!(&s.kind, SyntaxKind::Symbol(n) if n == "&"));
                if second {
                    return Err(format!("{}: multiple & in pattern", span));
                }
                let remaining = &items[pos + 1..];
                if remaining.len() != 1 {
                    return Err(format!(
                        "{}: & must be followed by exactly one pattern",
                        span
                    ));
                }
                Ok((&items[..pos], Some(&remaining[0])))
            }
        }
    }

    /// Convert a syntax pattern into an HirPattern, creating bindings for each leaf symbol.
    /// `scope` determines whether bindings are Local or Global.
    /// `immutable` determines whether bindings are marked immutable (def vs var).
    pub(super) fn analyze_destructure_pattern(
        &mut self,
        syntax: &Syntax,
        scope: BindingScope,
        immutable: bool,
        span: &Span,
    ) -> Result<HirPattern, String> {
        match &syntax.kind {
            SyntaxKind::Symbol(name) if name == "_" => Ok(HirPattern::Wildcard),
            SyntaxKind::Symbol(name) => {
                let in_function = self.scopes.iter().any(|s| s.is_function);
                let binding_scope = if in_function {
                    BindingScope::Local
                } else {
                    scope
                };

                let binding = if in_function {
                    // Check if pre-created by analyze_begin
                    let name_scopes = syntax.scopes.as_slice();
                    if let Some(existing) = self.lookup_in_current_scope(name, name_scopes) {
                        existing
                    } else {
                        self.bind(name, name_scopes, binding_scope)
                    }
                } else if matches!(binding_scope, BindingScope::Global) {
                    self.bind(name, &[], binding_scope)
                } else {
                    self.bind(name, syntax.scopes.as_slice(), binding_scope)
                };

                if immutable {
                    binding.mark_immutable();
                }
                Ok(HirPattern::Var(binding))
            }
            SyntaxKind::List(items) => {
                let (fixed, rest_syntax) = Self::split_rest_pattern(items, span)?;
                let mut elements = Vec::new();
                for item in fixed {
                    elements.push(self.analyze_destructure_pattern(item, scope, immutable, span)?);
                }
                let rest = match rest_syntax {
                    Some(r) => Some(Box::new(
                        self.analyze_destructure_pattern(r, scope, immutable, span)?,
                    )),
                    None => None,
                };
                Ok(HirPattern::List { elements, rest })
            }
            SyntaxKind::Tuple(items) | SyntaxKind::Array(items) => {
                // Both [...] and @[...] destructure the same way in binding forms
                // (no type guard — ArrayRefOrNil handles both)
                let (fixed, rest_syntax) = Self::split_rest_pattern(items, span)?;
                let mut elements = Vec::new();
                for item in fixed {
                    elements.push(self.analyze_destructure_pattern(item, scope, immutable, span)?);
                }
                let rest = match rest_syntax {
                    Some(r) => Some(Box::new(
                        self.analyze_destructure_pattern(r, scope, immutable, span)?,
                    )),
                    None => None,
                };
                Ok(HirPattern::Tuple { elements, rest })
            }
            SyntaxKind::Struct(items) | SyntaxKind::Table(items) => {
                // Both {...} and @{...} destructure the same way in binding forms
                // (no type guard — TableGetOrNil handles both)
                if items.len() % 2 != 0 {
                    return Err(format!(
                        "{}: struct/table destructuring requires keyword-pattern pairs",
                        span
                    ));
                }
                let mut entries = Vec::new();
                for pair in items.chunks(2) {
                    let key_name = match &pair[0].kind {
                        SyntaxKind::Keyword(k) => k.clone(),
                        _ => {
                            return Err(format!(
                                "{}: struct/table destructuring key must be a keyword, got {}",
                                span, pair[0]
                            ))
                        }
                    };
                    let pattern =
                        self.analyze_destructure_pattern(&pair[1], scope, immutable, span)?;
                    entries.push((key_name, pattern));
                }
                Ok(HirPattern::Struct { entries })
            }
            _ => Err(format!(
                "{}: destructuring pattern element must be a symbol, list, tuple, array, struct, or table",
                span
            )),
        }
    }
}
