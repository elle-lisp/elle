//! Hygienic macro expansion

use super::{ScopeId, Span, Syntax, SyntaxKind};
use std::collections::HashMap;

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
}

impl Expander {
    pub fn new() -> Self {
        Expander {
            macros: HashMap::new(),
            next_scope_id: 1, // 0 is reserved for top-level
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

    /// Resolve a qualified symbol like `string:upcase` to its flat primitive name.
    /// Returns None if the symbol is not qualified or the module is unknown.
    fn resolve_qualified_symbol(&self, name: &str) -> Option<String> {
        // Check if it's a qualified name (contains ':' but doesn't start with ':')
        if name.starts_with(':') || !name.contains(':') {
            return None;
        }

        let parts: Vec<&str> = name.splitn(2, ':').collect();
        if parts.len() != 2 {
            return None;
        }

        let module = parts[0];
        let func = parts[1];

        // Module-specific resolution rules
        match module {
            "string" => {
                // string:upcase -> string-upcase, string:length -> string-length, etc.
                Some(format!("string-{}", func))
            }
            "math" => {
                // math:abs -> abs, math:floor -> floor, etc.
                // Math functions are registered with their short names
                Some(func.to_string())
            }
            "list" => {
                // list:length -> length, list:append -> append, etc.
                // List functions are registered with their short names
                Some(func.to_string())
            }
            "json" => {
                // json:parse -> json-parse, json:serialize -> json-serialize
                Some(format!("json-{}", func))
            }
            _ => None, // Unknown module
        }
    }

    /// Expand all macros in a syntax tree
    pub fn expand(&mut self, syntax: Syntax) -> Result<Syntax, String> {
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
                        return self.handle_thread_first(items, &syntax.span);
                    }
                    if name == "->>" {
                        return self.handle_thread_last(items, &syntax.span);
                    }

                    // Handle macro introspection
                    if name == "macro?" {
                        return self.handle_macro_predicate(items, &syntax.span);
                    }
                    if name == "expand-macro" {
                        return self.handle_expand_macro(items, &syntax.span);
                    }

                    // Check if it's a macro call
                    if let Some(macro_def) = self.macros.get(name).cloned() {
                        return self.expand_macro_call(&macro_def, &items[1..], &syntax);
                    }
                }
                // Not a macro call - expand children recursively
                self.expand_list(items, syntax.span, syntax.scopes)
            }
            SyntaxKind::Vector(items) => self.expand_vector(items, syntax.span, syntax.scopes),
            SyntaxKind::Quote(_) => {
                // Don't expand inside quote
                Ok(syntax)
            }
            SyntaxKind::Quasiquote(inner) => {
                // Convert quasiquote to code that builds the structure
                self.quasiquote_to_code(inner, 1, &syntax.span)
            }
            _ => Ok(syntax),
        }
    }

    /// Handle (defmacro name (params...) body) or (define-macro name (params...) body)
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

    fn expand_macro_call(
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
    fn eval_quasiquote_to_syntax(&self, syntax: &Syntax) -> Result<Syntax, String> {
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

    fn substitute(&self, template: &Syntax, params: &[String], args: &[Syntax]) -> Syntax {
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

    fn substitute_quasiquote(
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

    fn add_scope_recursive(&self, mut syntax: Syntax, scope: ScopeId) -> Syntax {
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
            other => other,
        };

        syntax
    }

    /// Handle thread-first macro: (-> value form1 form2 ...)
    /// Inserts value as the FIRST argument to each form
    fn handle_thread_first(&mut self, items: &[Syntax], span: &Span) -> Result<Syntax, String> {
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
    fn handle_thread_last(&mut self, items: &[Syntax], span: &Span) -> Result<Syntax, String> {
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

    /// Handle (macro? symbol) - returns #t if symbol is a defined macro, #f otherwise
    ///
    /// This is handled at expansion time because:
    /// 1. The Expander knows which macros are defined
    /// 2. The symbol would otherwise be resolved as a variable by the analyzer
    fn handle_macro_predicate(&self, items: &[Syntax], span: &Span) -> Result<Syntax, String> {
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
    fn handle_expand_macro(&mut self, items: &[Syntax], span: &Span) -> Result<Syntax, String> {
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

    fn expand_list(
        &mut self,
        items: &[Syntax],
        span: Span,
        scopes: Vec<ScopeId>,
    ) -> Result<Syntax, String> {
        let expanded: Result<Vec<Syntax>, String> =
            items.iter().map(|item| self.expand(item.clone())).collect();
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
    ) -> Result<Syntax, String> {
        let expanded: Result<Vec<Syntax>, String> =
            items.iter().map(|item| self.expand(item.clone())).collect();
        Ok(Syntax::with_scopes(
            SyntaxKind::Vector(expanded?),
            span,
            scopes,
        ))
    }

    /// Convert quasiquote to code that constructs the value at runtime
    /// depth tracks nesting level for nested quasiquotes
    fn quasiquote_to_code(
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
    fn quasiquote_list_to_code(
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

impl Default for Expander {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quasiquote_simple_list() {
        let mut expander = Expander::new();
        let span = Span::new(0, 10, 1, 1);

        // `(a b c)
        let items = vec![
            Syntax::new(SyntaxKind::Symbol("a".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Symbol("b".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Symbol("c".to_string()), span.clone()),
        ];
        let syntax = Syntax::new(
            SyntaxKind::Quasiquote(Box::new(Syntax::new(SyntaxKind::List(items), span.clone()))),
            span.clone(),
        );

        let result = expander.expand(syntax).unwrap();
        // Should expand to (list (quote a) (quote b) (quote c))
        let result_str = result.to_string();
        assert!(
            result_str.contains("list"),
            "Result should contain 'list': {}",
            result_str
        );
        assert!(
            result_str.contains("quote"),
            "Result should contain 'quote': {}",
            result_str
        );
    }

    #[test]
    fn test_quasiquote_with_unquote() {
        let mut expander = Expander::new();
        let span = Span::new(0, 10, 1, 1);

        // `(a ,x b)
        let items = vec![
            Syntax::new(SyntaxKind::Symbol("a".to_string()), span.clone()),
            Syntax::new(
                SyntaxKind::Unquote(Box::new(Syntax::new(
                    SyntaxKind::Symbol("x".to_string()),
                    span.clone(),
                ))),
                span.clone(),
            ),
            Syntax::new(SyntaxKind::Symbol("b".to_string()), span.clone()),
        ];
        let syntax = Syntax::new(
            SyntaxKind::Quasiquote(Box::new(Syntax::new(SyntaxKind::List(items), span.clone()))),
            span.clone(),
        );

        let result = expander.expand(syntax).unwrap();
        let result_str = result.to_string();
        assert!(
            result_str.contains("list"),
            "Result should contain 'list': {}",
            result_str
        );
        assert!(
            result_str.contains("quote"),
            "Result should contain 'quote': {}",
            result_str
        );
        assert!(
            result_str.contains("x"),
            "Result should contain 'x': {}",
            result_str
        );
    }

    #[test]
    fn test_quasiquote_with_splicing() {
        let mut expander = Expander::new();
        let span = Span::new(0, 10, 1, 1);

        // `(a ,@xs b)
        let items = vec![
            Syntax::new(SyntaxKind::Symbol("a".to_string()), span.clone()),
            Syntax::new(
                SyntaxKind::UnquoteSplicing(Box::new(Syntax::new(
                    SyntaxKind::Symbol("xs".to_string()),
                    span.clone(),
                ))),
                span.clone(),
            ),
            Syntax::new(SyntaxKind::Symbol("b".to_string()), span.clone()),
        ];
        let syntax = Syntax::new(
            SyntaxKind::Quasiquote(Box::new(Syntax::new(SyntaxKind::List(items), span.clone()))),
            span.clone(),
        );

        let result = expander.expand(syntax).unwrap();
        let result_str = result.to_string();
        assert!(
            result_str.contains("append"),
            "Result should contain 'append': {}",
            result_str
        );
        assert!(
            result_str.contains("list"),
            "Result should contain 'list': {}",
            result_str
        );
        assert!(
            result_str.contains("xs"),
            "Result should contain 'xs': {}",
            result_str
        );
    }

    #[test]
    fn test_quasiquote_non_list() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // `x
        let syntax = Syntax::new(
            SyntaxKind::Quasiquote(Box::new(Syntax::new(
                SyntaxKind::Symbol("x".to_string()),
                span.clone(),
            ))),
            span.clone(),
        );

        let result = expander.expand(syntax).unwrap();
        let result_str = result.to_string();
        // Should expand to (quote x)
        assert!(
            result_str.contains("quote"),
            "Result should contain 'quote': {}",
            result_str
        );
        assert!(
            result_str.contains("x"),
            "Result should contain 'x': {}",
            result_str
        );
    }

    #[test]
    fn test_defmacro_registration() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // Define a macro using defmacro: (defmacro double (x) (* x 2))
        let defmacro_form = Syntax::new(
            SyntaxKind::List(vec![
                Syntax::new(SyntaxKind::Symbol("defmacro".to_string()), span.clone()),
                Syntax::new(SyntaxKind::Symbol("double".to_string()), span.clone()),
                Syntax::new(
                    SyntaxKind::List(vec![Syntax::new(
                        SyntaxKind::Symbol("x".to_string()),
                        span.clone(),
                    )]),
                    span.clone(),
                ),
                Syntax::new(
                    SyntaxKind::List(vec![
                        Syntax::new(SyntaxKind::Symbol("*".to_string()), span.clone()),
                        Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone()),
                        Syntax::new(SyntaxKind::Int(2), span.clone()),
                    ]),
                    span.clone(),
                ),
            ]),
            span.clone(),
        );

        let result = expander.expand(defmacro_form);
        assert!(result.is_ok());
        let expanded = result.unwrap();
        // defmacro should expand to nil
        assert_eq!(expanded.to_string(), "nil");

        // Now use the macro: (double 21)
        let macro_call = Syntax::new(
            SyntaxKind::List(vec![
                Syntax::new(SyntaxKind::Symbol("double".to_string()), span.clone()),
                Syntax::new(SyntaxKind::Int(21), span.clone()),
            ]),
            span,
        );

        let result = expander.expand(macro_call);
        assert!(result.is_ok());
        let expanded = result.unwrap();
        // Should expand to (* 21 2)
        assert_eq!(expanded.to_string(), "(* 21 2)");
    }

    #[test]
    fn test_defmacro_invalid_syntax() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // defmacro with wrong number of arguments
        let defmacro_form = Syntax::new(
            SyntaxKind::List(vec![
                Syntax::new(SyntaxKind::Symbol("defmacro".to_string()), span.clone()),
                Syntax::new(SyntaxKind::Symbol("double".to_string()), span.clone()),
            ]),
            span.clone(),
        );

        let result = expander.expand(defmacro_form);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("requires exactly 3 arguments"));
    }

    #[test]
    fn test_defmacro_non_symbol_name() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // defmacro with non-symbol name
        let defmacro_form = Syntax::new(
            SyntaxKind::List(vec![
                Syntax::new(SyntaxKind::Symbol("defmacro".to_string()), span.clone()),
                Syntax::new(SyntaxKind::Int(42), span.clone()),
                Syntax::new(SyntaxKind::List(vec![]), span.clone()),
                Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone()),
            ]),
            span.clone(),
        );

        let result = expander.expand(defmacro_form);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("macro name must be a symbol"));
    }

    #[test]
    fn test_macro_predicate_true() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // Define a macro
        let macro_def = MacroDef {
            name: "my-macro".to_string(),
            params: vec!["x".to_string()],
            template: Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone()),
            definition_scope: ScopeId(0),
        };
        expander.define_macro(macro_def);

        // (macro? my-macro) should return #t
        let check = Syntax::new(
            SyntaxKind::List(vec![
                Syntax::new(SyntaxKind::Symbol("macro?".to_string()), span.clone()),
                Syntax::new(SyntaxKind::Symbol("my-macro".to_string()), span.clone()),
            ]),
            span,
        );

        let result = expander.expand(check);
        assert!(result.is_ok());
        let expanded = result.unwrap();
        assert_eq!(expanded.to_string(), "#t");
    }

    #[test]
    fn test_macro_predicate_false() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // (macro? not-a-macro) should return #f
        let check = Syntax::new(
            SyntaxKind::List(vec![
                Syntax::new(SyntaxKind::Symbol("macro?".to_string()), span.clone()),
                Syntax::new(SyntaxKind::Symbol("not-a-macro".to_string()), span.clone()),
            ]),
            span,
        );

        let result = expander.expand(check);
        assert!(result.is_ok());
        let expanded = result.unwrap();
        assert_eq!(expanded.to_string(), "#f");
    }

    #[test]
    fn test_macro_predicate_non_symbol() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // (macro? 42) should return #f (not a symbol)
        let check = Syntax::new(
            SyntaxKind::List(vec![
                Syntax::new(SyntaxKind::Symbol("macro?".to_string()), span.clone()),
                Syntax::new(SyntaxKind::Int(42), span.clone()),
            ]),
            span,
        );

        let result = expander.expand(check);
        assert!(result.is_ok());
        let expanded = result.unwrap();
        assert_eq!(expanded.to_string(), "#f");
    }

    #[test]
    fn test_macro_predicate_wrong_arity() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // (macro?) with no arguments should error
        let check = Syntax::new(
            SyntaxKind::List(vec![Syntax::new(
                SyntaxKind::Symbol("macro?".to_string()),
                span.clone(),
            )]),
            span,
        );

        let result = expander.expand(check);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("requires exactly 1 argument"));
    }

    #[test]
    fn test_expand_macro_basic() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // Define a macro: (defmacro double (x) (+ x x))
        let template = Syntax::new(
            SyntaxKind::List(vec![
                Syntax::new(SyntaxKind::Symbol("+".to_string()), span.clone()),
                Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone()),
                Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone()),
            ]),
            span.clone(),
        );
        let macro_def = MacroDef {
            name: "double".to_string(),
            params: vec!["x".to_string()],
            template,
            definition_scope: ScopeId(0),
        };
        expander.define_macro(macro_def);

        // (expand-macro '(double 5)) should return '(+ 5 5)
        let expand_call = Syntax::new(
            SyntaxKind::List(vec![
                Syntax::new(SyntaxKind::Symbol("expand-macro".to_string()), span.clone()),
                Syntax::new(
                    SyntaxKind::Quote(Box::new(Syntax::new(
                        SyntaxKind::List(vec![
                            Syntax::new(SyntaxKind::Symbol("double".to_string()), span.clone()),
                            Syntax::new(SyntaxKind::Int(5), span.clone()),
                        ]),
                        span.clone(),
                    ))),
                    span.clone(),
                ),
            ]),
            span,
        );

        let result = expander.expand(expand_call);
        assert!(result.is_ok());
        let expanded = result.unwrap();
        // Result should be a quoted form: '(+ 5 5)
        assert_eq!(expanded.to_string(), "'(+ 5 5)");
    }

    #[test]
    fn test_expand_macro_non_macro() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // (expand-macro '(+ 1 2)) should return '(+ 1 2) unchanged
        let expand_call = Syntax::new(
            SyntaxKind::List(vec![
                Syntax::new(SyntaxKind::Symbol("expand-macro".to_string()), span.clone()),
                Syntax::new(
                    SyntaxKind::Quote(Box::new(Syntax::new(
                        SyntaxKind::List(vec![
                            Syntax::new(SyntaxKind::Symbol("+".to_string()), span.clone()),
                            Syntax::new(SyntaxKind::Int(1), span.clone()),
                            Syntax::new(SyntaxKind::Int(2), span.clone()),
                        ]),
                        span.clone(),
                    ))),
                    span.clone(),
                ),
            ]),
            span,
        );

        let result = expander.expand(expand_call);
        assert!(result.is_ok());
        let expanded = result.unwrap();
        // Result should be unchanged: '(+ 1 2)
        assert_eq!(expanded.to_string(), "'(+ 1 2)");
    }

    #[test]
    fn test_expand_macro_wrong_arity() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // (expand-macro) with no arguments should error
        let expand_call = Syntax::new(
            SyntaxKind::List(vec![Syntax::new(
                SyntaxKind::Symbol("expand-macro".to_string()),
                span.clone(),
            )]),
            span,
        );

        let result = expander.expand(expand_call);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("requires exactly 1 argument"));
    }

    #[test]
    fn test_expand_macro_unquoted_arg() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // (expand-macro x) with unquoted arg returns the arg unchanged
        let expand_call = Syntax::new(
            SyntaxKind::List(vec![
                Syntax::new(SyntaxKind::Symbol("expand-macro".to_string()), span.clone()),
                Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone()),
            ]),
            span,
        );

        let result = expander.expand(expand_call);
        assert!(result.is_ok());
        let expanded = result.unwrap();
        // Result should be the symbol x unchanged
        assert_eq!(expanded.to_string(), "x");
    }

    #[test]
    fn test_qualified_symbol_string_module() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // string:upcase should expand to string-upcase
        let syntax = Syntax::new(
            SyntaxKind::Symbol("string:upcase".to_string()),
            span.clone(),
        );
        let result = expander.expand(syntax).unwrap();
        assert_eq!(result.to_string(), "string-upcase");

        // string:length should expand to string-length
        let syntax = Syntax::new(SyntaxKind::Symbol("string:length".to_string()), span);
        let result = expander.expand(syntax).unwrap();
        assert_eq!(result.to_string(), "string-length");
    }

    #[test]
    fn test_qualified_symbol_math_module() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // math:abs should expand to abs
        let syntax = Syntax::new(SyntaxKind::Symbol("math:abs".to_string()), span.clone());
        let result = expander.expand(syntax).unwrap();
        assert_eq!(result.to_string(), "abs");

        // math:floor should expand to floor
        let syntax = Syntax::new(SyntaxKind::Symbol("math:floor".to_string()), span);
        let result = expander.expand(syntax).unwrap();
        assert_eq!(result.to_string(), "floor");
    }

    #[test]
    fn test_qualified_symbol_list_module() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // list:length should expand to length
        let syntax = Syntax::new(SyntaxKind::Symbol("list:length".to_string()), span.clone());
        let result = expander.expand(syntax).unwrap();
        assert_eq!(result.to_string(), "length");

        // list:append should expand to append
        let syntax = Syntax::new(SyntaxKind::Symbol("list:append".to_string()), span);
        let result = expander.expand(syntax).unwrap();
        assert_eq!(result.to_string(), "append");
    }

    #[test]
    fn test_qualified_symbol_in_call() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // (string:upcase "hello") should expand to (string-upcase "hello")
        let syntax = Syntax::new(
            SyntaxKind::List(vec![
                Syntax::new(
                    SyntaxKind::Symbol("string:upcase".to_string()),
                    span.clone(),
                ),
                Syntax::new(SyntaxKind::String("hello".to_string()), span.clone()),
            ]),
            span,
        );
        let result = expander.expand(syntax).unwrap();
        assert_eq!(result.to_string(), "(string-upcase \"hello\")");
    }

    #[test]
    fn test_qualified_symbol_unknown_module() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // unknown:foo should remain unchanged (unknown module)
        let syntax = Syntax::new(SyntaxKind::Symbol("unknown:foo".to_string()), span);
        let result = expander.expand(syntax).unwrap();
        assert_eq!(result.to_string(), "unknown:foo");
    }

    #[test]
    fn test_keyword_not_qualified() {
        let mut expander = Expander::new();
        let span = Span::new(0, 5, 1, 1);

        // :keyword should remain a keyword, not be treated as qualified
        let syntax = Syntax::new(SyntaxKind::Keyword("foo".to_string()), span);
        let result = expander.expand(syntax).unwrap();
        // Keywords are stored without the leading colon in SyntaxKind::Keyword
        assert!(matches!(result.kind, SyntaxKind::Keyword(ref s) if s == "foo"));
    }
}
