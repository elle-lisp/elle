//! Hygienic macro expansion

mod introspection;
mod macro_expand;
mod qualified;
mod quasiquote;
mod threading;

#[cfg(test)]
mod tests;

use super::{ScopeId, Span, Syntax, SyntaxKind};
use crate::symbol::SymbolTable;
use crate::vm::VM;
use std::collections::HashMap;

/// Maximum macro expansion depth before erroring (prevents infinite expansion)
const MAX_MACRO_EXPANSION_DEPTH: usize = 200;

/// Macro definition stored as Syntax
#[derive(Debug, Clone)]
pub struct MacroDef {
    pub name: String,
    pub params: Vec<String>,
    pub template: Syntax,
    pub definition_scope: ScopeId,
}

/// Hygienic macro expander
pub struct Expander {
    macros: HashMap<String, MacroDef>,
    next_scope_id: u32,
    expansion_depth: usize,
}

impl Expander {
    pub fn new() -> Self {
        Expander {
            macros: HashMap::new(),
            next_scope_id: 1, // 0 is reserved for top-level
            expansion_depth: 0,
        }
    }

    /// Register a macro definition
    pub fn define_macro(&mut self, def: MacroDef) {
        self.macros.insert(def.name.clone(), def);
    }

    /// Generate a fresh scope ID
    pub fn fresh_scope(&mut self) -> ScopeId {
        let id = ScopeId(self.next_scope_id);
        self.next_scope_id += 1;
        id
    }

    /// Create a symbol syntax node
    fn make_symbol(&self, name: &str, span: Span) -> Syntax {
        Syntax::new(SyntaxKind::Symbol(name.to_string()), span)
    }

    /// Create a list syntax node
    fn make_list(&self, items: Vec<Syntax>, span: Span) -> Syntax {
        Syntax::new(SyntaxKind::List(items), span)
    }

    /// Expand all macros in a syntax tree
    pub fn expand(
        &mut self,
        syntax: Syntax,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        match &syntax.kind {
            SyntaxKind::Symbol(name) => {
                // Resolve qualified symbols like string:upcase -> string-upcase
                if let Some(resolved) = self.resolve_qualified_symbol(name) {
                    Ok(Syntax::with_scopes(
                        SyntaxKind::Symbol(resolved),
                        syntax.span,
                        syntax.scopes,
                    ))
                } else {
                    Ok(syntax)
                }
            }
            SyntaxKind::List(items) if !items.is_empty() => {
                // Check if first element is a symbol
                if let Some(name) = items[0].as_symbol() {
                    // Handle defmacro specially - register and return nil
                    if name == "defmacro" || name == "define-macro" {
                        return self.handle_defmacro(items, &syntax.span);
                    }

                    // Handle threading macros
                    if name == "->" {
                        return self.handle_thread_first(items, &syntax.span, symbols, vm);
                    }
                    if name == "->>" {
                        return self.handle_thread_last(items, &syntax.span, symbols, vm);
                    }

                    // Handle macro introspection
                    if name == "macro?" {
                        return self.handle_macro_predicate(items, &syntax.span);
                    }
                    if name == "expand-macro" {
                        return self.handle_expand_macro(items, &syntax.span, symbols, vm);
                    }

                    // Handle (var (f x y) body...) and (def (f x y) body...) shorthand
                    // Desugar to (var/def f (fn (x y) body...))
                    if (name == "var" || name == "def") && items.len() >= 3 {
                        if let SyntaxKind::List(name_and_params) = &items[1].kind {
                            if !name_and_params.is_empty() {
                                let func_name = name_and_params[0].clone();
                                let params = Syntax::new(
                                    SyntaxKind::List(name_and_params[1..].to_vec()),
                                    items[1].span.clone(),
                                );
                                let fn_sym = Syntax::new(
                                    SyntaxKind::Symbol("fn".to_string()),
                                    items[1].span.clone(),
                                );
                                let mut lambda_parts = vec![fn_sym, params];
                                lambda_parts.extend(items[2..].iter().cloned());
                                let lambda = Syntax::new(
                                    SyntaxKind::List(lambda_parts),
                                    syntax.span.clone(),
                                );
                                let binding_sym = items[0].clone();
                                let desugared = Syntax::new(
                                    SyntaxKind::List(vec![binding_sym, func_name, lambda]),
                                    syntax.span.clone(),
                                );
                                return self.expand(desugared, symbols, vm);
                            }
                        }
                    }

                    // Check if it's a macro call
                    if let Some(macro_def) = self.macros.get(name).cloned() {
                        return self.expand_macro_call(
                            &macro_def,
                            &items[1..],
                            &syntax,
                            symbols,
                            vm,
                        );
                    }
                }
                // Not a macro call - expand children recursively
                self.expand_list(items, syntax.span, syntax.scopes, symbols, vm)
            }
            SyntaxKind::Vector(items) => {
                self.expand_vector(items, syntax.span, syntax.scopes, symbols, vm)
            }
            SyntaxKind::Quote(_) => {
                // Don't expand inside quote
                Ok(syntax)
            }
            SyntaxKind::Quasiquote(inner) => {
                // Convert quasiquote to code that builds the structure
                self.quasiquote_to_code(inner, 1, &syntax.span, symbols, vm)
            }
            _ => Ok(syntax),
        }
    }

    /// Handle (defmacro name (params...) body) or (var-macro name (params...) body)
    fn handle_defmacro(&mut self, items: &[Syntax], span: &Span) -> Result<Syntax, String> {
        // Syntax: (defmacro name (params...) body)
        if items.len() != 4 {
            return Err(format!(
                "{}: defmacro requires exactly 3 arguments (name, parameters, body)",
                span
            ));
        }

        // Get macro name
        let name = items[1]
            .as_symbol()
            .ok_or_else(|| format!("{}: macro name must be a symbol", span))?
            .to_string();

        // Get parameter list
        let params_syntax = items[2]
            .as_list()
            .ok_or_else(|| format!("{}: macro parameters must be a list", span))?;

        let params: Vec<String> = params_syntax
            .iter()
            .map(|p| {
                p.as_symbol()
                    .ok_or_else(|| format!("{}: macro parameter must be a symbol", span))
                    .map(|s| s.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Get the body template
        let template = items[3].clone();

        // Create and register the macro
        let macro_def = MacroDef {
            name: name.clone(),
            params,
            template,
            definition_scope: ScopeId(0), // Top-level scope
        };

        self.define_macro(macro_def);

        // Return nil - the macro definition itself doesn't produce code
        Ok(Syntax::new(SyntaxKind::Nil, span.clone()))
    }

    fn add_scope_recursive(&self, mut syntax: Syntax, scope: ScopeId) -> Syntax {
        // datum->syntax nodes keep their exact scopes — don't add intro scope
        if syntax.scope_exempt {
            return syntax;
        }

        // Add scope to this node
        syntax.add_scope(scope);

        // Recurse into children
        syntax.kind = match syntax.kind {
            SyntaxKind::List(items) => SyntaxKind::List(
                items
                    .into_iter()
                    .map(|item| self.add_scope_recursive(item, scope))
                    .collect(),
            ),
            SyntaxKind::Vector(items) => SyntaxKind::Vector(
                items
                    .into_iter()
                    .map(|item| self.add_scope_recursive(item, scope))
                    .collect(),
            ),
            SyntaxKind::Quote(inner) => {
                // Don't add scope inside quote - it's literal data
                SyntaxKind::Quote(inner)
            }
            SyntaxKind::Quasiquote(inner) => {
                SyntaxKind::Quasiquote(Box::new(self.add_scope_recursive(*inner, scope)))
            }
            SyntaxKind::Unquote(inner) => {
                SyntaxKind::Unquote(Box::new(self.add_scope_recursive(*inner, scope)))
            }
            SyntaxKind::UnquoteSplicing(inner) => {
                SyntaxKind::UnquoteSplicing(Box::new(self.add_scope_recursive(*inner, scope)))
            }
            // Don't recurse into syntax literals — the inner Value::syntax
            // already carries its correct scopes from the original context.
            SyntaxKind::SyntaxLiteral(_) => syntax.kind,
            other => other,
        };

        syntax
    }

    fn expand_list(
        &mut self,
        items: &[Syntax],
        span: Span,
        scopes: Vec<ScopeId>,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        let expanded: Result<Vec<Syntax>, String> = items
            .iter()
            .map(|item| self.expand(item.clone(), symbols, vm))
            .collect();
        Ok(Syntax::with_scopes(
            SyntaxKind::List(expanded?),
            span,
            scopes,
        ))
    }

    fn expand_vector(
        &mut self,
        items: &[Syntax],
        span: Span,
        scopes: Vec<ScopeId>,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        let expanded: Result<Vec<Syntax>, String> = items
            .iter()
            .map(|item| self.expand(item.clone(), symbols, vm))
            .collect();
        Ok(Syntax::with_scopes(
            SyntaxKind::Vector(expanded?),
            span,
            scopes,
        ))
    }
}

impl Default for Expander {
    fn default() -> Self {
        Self::new()
    }
}
