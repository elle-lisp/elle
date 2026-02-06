//! Scope and symbol table management

use std::collections::HashMap;

/// Kind of symbol binding
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Variable,
    Parameter,
    PatternBinding,
}

/// Information about a bound symbol
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub kind: SymbolKind,
    pub arity: Option<usize>, // For functions
    pub line: usize,
    pub column: usize,
    pub used: bool,
}

/// A scope level in the linter context
pub struct Scope {
    symbols: HashMap<String, SymbolInfo>,
    parent: Option<Box<Scope>>,
}

impl Scope {
    pub fn new() -> Self {
        Self {
            symbols: HashMap::new(),
            parent: None,
        }
    }

    pub fn with_parent(parent: Scope) -> Self {
        Self {
            symbols: HashMap::new(),
            parent: Some(Box::new(parent)),
        }
    }

    pub fn define(&mut self, name: String, kind: SymbolKind, line: usize, column: usize) {
        self.symbols.insert(
            name,
            SymbolInfo {
                kind,
                arity: None,
                line,
                column,
                used: false,
            },
        );
    }

    pub fn define_function(&mut self, name: String, arity: usize, line: usize, column: usize) {
        self.symbols.insert(
            name,
            SymbolInfo {
                kind: SymbolKind::Function,
                arity: Some(arity),
                line,
                column,
                used: false,
            },
        );
    }

    pub fn lookup(&self, name: &str) -> Option<&SymbolInfo> {
        if let Some(info) = self.symbols.get(name) {
            Some(info)
        } else if let Some(parent) = &self.parent {
            parent.lookup(name)
        } else {
            None
        }
    }

    pub fn lookup_mut(&mut self, name: &str) -> Option<&mut SymbolInfo> {
        if self.symbols.contains_key(name) {
            self.symbols.get_mut(name)
        } else if let Some(parent) = &mut self.parent {
            parent.lookup_mut(name)
        } else {
            None
        }
    }

    pub fn mark_used(&mut self, name: &str) {
        if let Some(info) = self.lookup_mut(name) {
            info.used = true;
        }
    }

    pub fn get_unused(&self) -> Vec<(String, &SymbolInfo)> {
        self.symbols
            .iter()
            .filter(|(_, info)| !info.used)
            .map(|(name, info)| (name.clone(), info))
            .collect()
    }

    pub fn is_defined(&self, name: &str) -> bool {
        self.lookup(name).is_some()
    }

    pub fn all_symbols(&self) -> HashMap<String, SymbolInfo> {
        let mut result = self.symbols.clone();
        if let Some(parent) = &self.parent {
            let parent_symbols = parent.all_symbols();
            for (k, v) in parent_symbols {
                result.entry(k).or_insert(v);
            }
        }
        result
    }
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}

/// Built-in functions with their arity
pub fn builtin_arity(name: &str) -> Option<usize> {
    match name {
        // Arithmetic
        "+" | "-" | "*" | "/" | "mod" | "remainder" => Some(2), // Actually variadic, but min 2
        // Comparison
        "=" | "<" | ">" | "<=" | ">=" => Some(2),
        // List operations
        "list" => None, // Variadic
        "cons" => Some(2),
        "first" | "rest" => Some(1),
        "length" => Some(1),
        "append" => Some(2),
        "reverse" => Some(1),
        "nth" => Some(2),
        "last" => Some(1),
        "take" | "drop" => Some(2),
        // Math functions
        "abs" | "sqrt" | "sin" | "cos" | "tan" | "log" | "exp" | "floor" | "ceil" | "round" => {
            Some(1)
        }
        "pow" => Some(2),
        "min" | "max" => Some(2),
        // String operations
        "string-length" | "string-upcase" | "string-downcase" => Some(1),
        "string-append" => Some(2),
        "substring" => Some(3),
        "string-index" => Some(2),
        "char-at" => Some(2),
        // Type operations
        "type" => Some(1),
        "int" | "float" | "string" => Some(1),
        // Logic
        "not" => Some(1),
        "if" => Some(3),
        // Vector operations
        "vector" => None, // Variadic
        "vector-length" => Some(1),
        "vector-ref" => Some(2),
        "vector-set!" => Some(3),
        // Special forms
        "define" => None, // Variable arity
        "quote" => Some(1),
        "begin" => None, // Variadic
        "let" => None,
        "let*" => None,
        "fn" => None,
        "match" => None,
        "while" => None,
        "foreach" => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_define_and_lookup() {
        let mut scope = Scope::new();
        scope.define("x".to_string(), SymbolKind::Variable, 1, 1);

        assert!(scope.is_defined("x"));
        assert!(!scope.is_defined("y"));
    }

    #[test]
    fn test_scope_parent() {
        let mut parent = Scope::new();
        parent.define("x".to_string(), SymbolKind::Variable, 1, 1);

        let child = Scope::with_parent(parent);
        assert!(child.is_defined("x"));
    }

    #[test]
    fn test_builtin_arity() {
        assert_eq!(builtin_arity("+"), Some(2));
        assert_eq!(builtin_arity("cons"), Some(2));
        assert_eq!(builtin_arity("list"), None);
        assert_eq!(builtin_arity("undefined"), None);
    }
}
