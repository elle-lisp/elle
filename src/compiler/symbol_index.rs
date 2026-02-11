//! Symbol index for IDE features (hover, completion, go-to-definition)
//!
//! Extracts symbol information from compiled Expr trees to enable
//! Language Server Protocol features.

use super::ast::{Expr, ExprWithLoc};
use crate::reader::SourceLoc;
use crate::symbol::SymbolTable;
use crate::value::SymbolId;
use std::collections::{HashMap, HashSet};

/// Kind of symbol for IDE classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    /// User-defined function
    Function,
    /// Variable or binding
    Variable,
    /// Built-in primitive
    Builtin,
    /// Macro
    Macro,
    /// Module
    Module,
}

impl SymbolKind {
    /// LSP completion kind string
    pub fn lsp_kind(&self) -> &'static str {
        match self {
            SymbolKind::Function => "Function",
            SymbolKind::Variable => "Variable",
            SymbolKind::Builtin => "Class",
            SymbolKind::Macro => "Keyword",
            SymbolKind::Module => "Module",
        }
    }
}

/// Information about a symbol definition
#[derive(Debug, Clone)]
pub struct SymbolDef {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub location: Option<SourceLoc>,
    pub arity: Option<usize>,
    pub documentation: Option<String>,
}

impl SymbolDef {
    pub fn new(id: SymbolId, name: String, kind: SymbolKind) -> Self {
        Self {
            id,
            name,
            kind,
            location: None,
            arity: None,
            documentation: None,
        }
    }

    pub fn with_location(mut self, loc: SourceLoc) -> Self {
        self.location = Some(loc);
        self
    }

    pub fn with_arity(mut self, arity: usize) -> Self {
        self.arity = Some(arity);
        self
    }

    pub fn with_documentation(mut self, doc: String) -> Self {
        self.documentation = Some(doc);
        self
    }
}

/// Index of symbols extracted from a compiled Expr
#[derive(Debug, Clone)]
pub struct SymbolIndex {
    /// All symbol definitions (both builtins and user-defined)
    pub definitions: HashMap<SymbolId, SymbolDef>,

    /// Symbol locations for go-to-definition
    pub symbol_locations: HashMap<SymbolId, SourceLoc>,

    /// Symbol usages for find-references
    pub symbol_usages: HashMap<SymbolId, Vec<SourceLoc>>,

    /// All available symbols for completion, grouped by kind
    pub available_symbols: Vec<(String, SymbolId, SymbolKind)>,
}

impl SymbolIndex {
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
            symbol_locations: HashMap::new(),
            symbol_usages: HashMap::new(),
            available_symbols: Vec::new(),
        }
    }

    /// Get documentation for a symbol
    pub fn get_documentation(&self, sym_id: SymbolId) -> Option<&str> {
        self.definitions
            .get(&sym_id)
            .and_then(|def| def.documentation.as_deref())
    }

    /// Get arity of a function
    pub fn get_arity(&self, sym_id: SymbolId) -> Option<usize> {
        self.definitions.get(&sym_id).and_then(|def| def.arity)
    }

    /// Get kind of symbol
    pub fn get_kind(&self, sym_id: SymbolId) -> Option<SymbolKind> {
        self.definitions.get(&sym_id).map(|def| def.kind)
    }

    /// Find symbol at a location (line, col)
    /// This would require source mapping which we'll implement in the LSP handler
    /// For now, this is a placeholder
    #[allow(unused)]
    pub fn find_symbol_at(&self, _line: usize, _col: usize) -> Option<SymbolId> {
        None
    }
}

/// Extract symbol index from compiled expressions
pub fn extract_symbols(exprs: &[ExprWithLoc], symbols: &SymbolTable) -> SymbolIndex {
    let mut index = SymbolIndex::new();
    let mut extractor = SymbolExtractor::new();

    for expr in exprs {
        extractor.walk_expr_with_loc(expr, &mut index, symbols);
    }

    // Add all available symbols to the index
    extractor.collect_available_symbols(&mut index, symbols);

    index
}

/// Helper to extract symbols from Expr tree
struct SymbolExtractor {
    seen_definitions: HashSet<SymbolId>,
}

impl SymbolExtractor {
    fn new() -> Self {
        Self {
            seen_definitions: HashSet::new(),
        }
    }

    fn walk_expr_with_loc(
        &mut self,
        expr_with_loc: &ExprWithLoc,
        index: &mut SymbolIndex,
        symbols: &SymbolTable,
    ) {
        self.walk_expr(&expr_with_loc.expr, &expr_with_loc.loc, index, symbols);
    }

    fn walk_expr(
        &mut self,
        expr: &Expr,
        loc: &Option<SourceLoc>,
        index: &mut SymbolIndex,
        symbols: &SymbolTable,
    ) {
        match expr {
            Expr::Literal(_) => {}

            Expr::Var(sym, _, _) => {
                if let Some(source_loc) = loc {
                    index
                        .symbol_usages
                        .entry(*sym)
                        .or_default()
                        .push(source_loc.clone());
                }
            }

            Expr::GlobalVar(sym) => {
                if let Some(source_loc) = loc {
                    index
                        .symbol_usages
                        .entry(*sym)
                        .or_default()
                        .push(source_loc.clone());
                }
            }

            Expr::Define { name, value } => {
                // Record the definition
                if let Some(source_loc) = loc {
                    index.symbol_locations.insert(*name, source_loc.clone());
                }

                if !self.seen_definitions.contains(name) {
                    self.seen_definitions.insert(*name);

                    if let Some(name_str) = symbols.name(*name) {
                        let def = SymbolDef::new(*name, name_str.to_string(), SymbolKind::Variable)
                            .with_location(
                                loc.as_ref()
                                    .cloned()
                                    .unwrap_or_else(|| SourceLoc::from_line_col(0, 0)),
                            );

                        index.definitions.insert(*name, def);
                    }
                }

                self.walk_expr(value, loc, index, symbols);
            }

            Expr::Lambda { body, params, .. } => {
                // Record parameters as variables
                for param in params {
                    if let Some(param_str) = symbols.name(*param) {
                        let def =
                            SymbolDef::new(*param, param_str.to_string(), SymbolKind::Variable)
                                .with_location(
                                    loc.as_ref()
                                        .cloned()
                                        .unwrap_or_else(|| SourceLoc::from_line_col(0, 0)),
                                );
                        index.definitions.insert(*param, def);
                    }
                }
                self.walk_expr(body, loc, index, symbols);
            }

            Expr::Let { bindings, body } => {
                // Record let bindings as variables
                for (var, init) in bindings {
                    if let Some(var_str) = symbols.name(*var) {
                        let def = SymbolDef::new(*var, var_str.to_string(), SymbolKind::Variable)
                            .with_location(
                                loc.as_ref()
                                    .cloned()
                                    .unwrap_or_else(|| SourceLoc::from_line_col(0, 0)),
                            );
                        index.definitions.insert(*var, def);
                    }
                    self.walk_expr(init, loc, index, symbols);
                }
                self.walk_expr(body, loc, index, symbols);
            }

            Expr::Letrec { bindings, body } => {
                // Record letrec bindings as functions
                for (var, init) in bindings {
                    if let Some(var_str) = symbols.name(*var) {
                        let def = SymbolDef::new(*var, var_str.to_string(), SymbolKind::Function)
                            .with_location(
                                loc.as_ref()
                                    .cloned()
                                    .unwrap_or_else(|| SourceLoc::from_line_col(0, 0)),
                            );
                        index.definitions.insert(*var, def);
                    }
                    self.walk_expr(init, loc, index, symbols);
                }
                self.walk_expr(body, loc, index, symbols);
            }

            Expr::If { cond, then, else_ } => {
                self.walk_expr(cond, loc, index, symbols);
                self.walk_expr(then, loc, index, symbols);
                self.walk_expr(else_, loc, index, symbols);
            }

            Expr::Cond { clauses, else_body } => {
                for (cond, body) in clauses {
                    self.walk_expr(cond, loc, index, symbols);
                    self.walk_expr(body, loc, index, symbols);
                }
                if let Some(else_body) = else_body {
                    self.walk_expr(else_body, loc, index, symbols);
                }
            }

            Expr::Begin(exprs) | Expr::Block(exprs) => {
                for e in exprs {
                    self.walk_expr(e, loc, index, symbols);
                }
            }

            Expr::Call { func, args, .. } => {
                self.walk_expr(func, loc, index, symbols);
                for arg in args {
                    self.walk_expr(arg, loc, index, symbols);
                }
            }

            Expr::While { cond, body } => {
                self.walk_expr(cond, loc, index, symbols);
                self.walk_expr(body, loc, index, symbols);
            }

            Expr::For { iter, body, .. } => {
                self.walk_expr(iter, loc, index, symbols);
                self.walk_expr(body, loc, index, symbols);
            }

            Expr::Match {
                value,
                patterns: _,
                default,
            } => {
                self.walk_expr(value, loc, index, symbols);
                if let Some(default) = default {
                    self.walk_expr(default, loc, index, symbols);
                }
            }

            Expr::Try {
                body,
                catch,
                finally,
            } => {
                self.walk_expr(body, loc, index, symbols);
                if let Some((_, handler)) = catch {
                    self.walk_expr(handler, loc, index, symbols);
                }
                if let Some(finally) = finally {
                    self.walk_expr(finally, loc, index, symbols);
                }
            }

            Expr::Throw { value } => {
                self.walk_expr(value, loc, index, symbols);
            }

            Expr::HandlerCase { body, handlers } => {
                self.walk_expr(body, loc, index, symbols);
                for (_exc_id, _var, handler_expr) in handlers {
                    self.walk_expr(handler_expr, loc, index, symbols);
                }
            }

            Expr::HandlerBind { handlers, body } => {
                self.walk_expr(body, loc, index, symbols);
                for (_exc_id, handler_fn) in handlers {
                    self.walk_expr(handler_fn, loc, index, symbols);
                }
            }

            Expr::Quote(_) | Expr::Quasiquote(_) | Expr::Unquote(_) => {
                // Don't walk quoted expressions
            }

            Expr::Set { value, .. } => {
                self.walk_expr(value, loc, index, symbols);
            }

            Expr::And(exprs) | Expr::Or(exprs) | Expr::Xor(exprs) => {
                for e in exprs {
                    self.walk_expr(e, loc, index, symbols);
                }
            }

            Expr::DefMacro { body, .. } => {
                self.walk_expr(body, loc, index, symbols);
            }

            Expr::Module { body, .. } => {
                self.walk_expr(body, loc, index, symbols);
            }

            Expr::Import { .. } | Expr::ModuleRef { .. } => {
                // Module references are handled elsewhere
            }
        }
    }

    fn collect_available_symbols(&self, index: &mut SymbolIndex, _symbols: &SymbolTable) {
        // Collect builtins and defined symbols
        for (sym_id, def) in &index.definitions {
            index
                .available_symbols
                .push((def.name.clone(), *sym_id, def.kind));
        }

        // Sort for consistent ordering
        index.available_symbols.sort_by(|a, b| a.0.cmp(&b.0));
    }
}

impl Default for SymbolIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// Hardcoded documentation for built-in primitives
pub fn get_primitive_documentation(name: &str) -> Option<&'static str> {
    Some(match name {
        // Arithmetic
        "+" => "Add numbers: (+ a b c ...)",
        "-" => "Subtract numbers: (- a b c ...)",
        "*" => "Multiply numbers: (* a b c ...)",
        "/" => "Divide numbers: (/ a b c ...)",
        "mod" => "Modulo operation: (mod a b)",
        "remainder" => "Remainder after division: (remainder a b)",

        // Comparison
        "=" => "Test equality: (= a b)",
        "<" => "Less than: (< a b)",
        ">" => "Greater than: (> a b)",
        "<=" => "Less than or equal: (<= a b)",
        ">=" => "Greater than or equal: (>= a b)",

        // List operations
        "cons" => "Construct a list: (cons head tail)",
        "first" => "Get first element: (first list)",
        "rest" => "Get rest of list: (rest list)",
        "length" => "Get list length: (length list)",
        "append" => "Append lists: (append list1 list2)",
        "reverse" => "Reverse a list: (reverse list)",
        "nth" => "Get nth element: (nth list index)",
        "last" => "Get last element: (last list)",
        "take" => "Take first n elements: (take list n)",
        "drop" => "Drop first n elements: (drop list n)",

        // Math functions
        "abs" => "Absolute value: (abs x)",
        "sqrt" => "Square root: (sqrt x)",
        "sin" => "Sine: (sin x)",
        "cos" => "Cosine: (cos x)",
        "tan" => "Tangent: (tan x)",
        "log" => "Natural logarithm: (log x)",
        "exp" => "Exponential: (exp x)",
        "floor" => "Floor: (floor x)",
        "ceil" => "Ceiling: (ceil x)",
        "round" => "Round: (round x)",
        "pow" => "Power: (pow base exponent)",
        "min" => "Minimum: (min a b)",
        "max" => "Maximum: (max a b)",

        // String operations
        "string-length" => "Get string length: (string-length s)",
        "string-upcase" => "Convert to uppercase: (string-upcase s)",
        "string-downcase" => "Convert to lowercase: (string-downcase s)",
        "string-append" => "Append strings: (string-append s1 s2)",
        "substring" => "Extract substring: (substring s start end)",
        "string-index" => "Find character index: (string-index s char)",
        "char-at" => "Get character at index: (char-at s index)",

        // Type operations
        "type" => "Get type of value: (type x)",

        // Logic
        "not" => "Logical NOT: (not x)",
        "if" => "Conditional: (if condition then else)",

        // Vector operations
        "vector-length" => "Get vector length: (vector-length v)",
        "vector-ref" => "Get vector element: (vector-ref v index)",
        "vector-set!" => "Set vector element: (vector-set! v index value)",

        // I/O
        "print" => "Print to output: (print x)",
        "println" => "Print with newline: (println x)",

        // Special forms
        "define" => "Define a variable: (define name value)",
        "quote" => "Quote expression: (quote expr)",
        "begin" => "Sequential execution: (begin expr1 expr2 ...)",
        "let" => "Local bindings: (let ((var val) ...) body)",
        "fn" => "Function definition: (fn (params ...) body)",
        "lambda" => "Function definition (alias for fn): (lambda (params ...) body)",
        "match" => "Pattern matching: (match value (pattern body) ...)",
        "while" => "Loop: (while condition body)",
        "foreach" => "Iterate: (foreach var iterable body)",

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_kind_lsp_kind() {
        assert_eq!(SymbolKind::Function.lsp_kind(), "Function");
        assert_eq!(SymbolKind::Variable.lsp_kind(), "Variable");
        assert_eq!(SymbolKind::Builtin.lsp_kind(), "Class");
    }

    #[test]
    fn test_symbol_def_builder() {
        let sym_id = SymbolId(1);
        let def = SymbolDef::new(sym_id, "test-var".to_string(), SymbolKind::Variable)
            .with_arity(2)
            .with_documentation("A test variable".to_string());

        assert_eq!(def.arity, Some(2));
        assert_eq!(def.documentation, Some("A test variable".to_string()));
    }

    #[test]
    fn test_primitive_documentation() {
        assert!(get_primitive_documentation("+").is_some());
        assert!(get_primitive_documentation("cons").is_some());
        assert!(get_primitive_documentation("nonexistent").is_none());
    }

    #[test]
    fn test_symbol_index_creation() {
        let index = SymbolIndex::new();
        assert_eq!(index.definitions.len(), 0);
        assert_eq!(index.available_symbols.len(), 0);
    }
}
