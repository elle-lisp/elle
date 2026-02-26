//! Hygienic macro expansion

mod introspection;
mod macro_expand;
mod qualified;
mod quasiquote;
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
    pub rest_param: Option<String>,
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

    /// Check if any macros are registered (used to detect if prelude is loaded)
    pub fn has_macros(&self) -> bool {
        !self.macros.is_empty()
    }

    /// Load the standard prelude macros.
    ///
    /// Parses and expands `prelude.lisp`, which registers macro
    /// definitions in this Expander. Must be called after the VM
    /// has primitives registered but before user code expansion.
    pub fn load_prelude(&mut self, symbols: &mut SymbolTable, vm: &mut VM) -> Result<(), String> {
        const PRELUDE: &str = include_str!("../../../prelude.lisp");
        let syntaxes = crate::reader::read_syntax_all(PRELUDE)?;
        for syntax in syntaxes {
            self.expand(syntax, symbols, vm)?;
        }
        Ok(())
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

                    // Handle macro introspection
                    if name == "macro?" {
                        return self.handle_macro_predicate(items, &syntax.span);
                    }
                    if name == "expand-macro" {
                        return self.handle_expand_macro(items, &syntax.span, symbols, vm);
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
            SyntaxKind::Array(items) => {
                self.expand_array(items, syntax.span, syntax.scopes, symbols, vm)
            }
            SyntaxKind::Table(items) => {
                self.expand_table(items, syntax.span, syntax.scopes, symbols, vm)
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
        let params_syntax = items[2].as_list().ok_or_else(|| {
            if matches!(items[2].kind, SyntaxKind::Array(_)) {
                format!(
                    "{}: macro parameters must use parentheses (params...), \
                     not brackets [...]",
                    items[2].span
                )
            } else {
                format!(
                    "{}: macro parameters must be a parenthesized list (params...), \
                     got {}",
                    items[2].span,
                    items[2].kind_label()
                )
            }
        })?;

        // Parse params, recognizing & as rest separator
        let mut fixed_params = Vec::new();
        let mut rest_param = None;
        let mut i = 0;
        while i < params_syntax.len() {
            let p = params_syntax[i]
                .as_symbol()
                .ok_or_else(|| format!("{}: macro parameter must be a symbol", span))?;
            if p == "&" {
                // Next symbol is the rest param
                if i + 1 >= params_syntax.len() {
                    return Err(format!("{}: expected parameter name after &", span));
                }
                if i + 2 < params_syntax.len() {
                    return Err(format!("{}: only one parameter allowed after &", span));
                }
                let rest_name = params_syntax[i + 1]
                    .as_symbol()
                    .ok_or_else(|| format!("{}: macro parameter must be a symbol", span))?;
                rest_param = Some(rest_name.to_string());
                break;
            }
            fixed_params.push(p.to_string());
            i += 1;
        }

        // Get the body template
        let template = items[3].clone();

        // Create and register the macro
        let macro_def = MacroDef {
            name: name.clone(),
            params: fixed_params,
            rest_param,
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
            SyntaxKind::Array(items) => SyntaxKind::Array(
                items
                    .into_iter()
                    .map(|item| self.add_scope_recursive(item, scope))
                    .collect(),
            ),
            SyntaxKind::Table(items) => SyntaxKind::Table(
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

    fn expand_array(
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
            SyntaxKind::Array(expanded?),
            span,
            scopes,
        ))
    }

    fn expand_table(
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
            SyntaxKind::Table(expanded?),
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
