//! Known effects of built-in primitives

use super::Effect;
use crate::symbol::SymbolTable;
use crate::value::SymbolId;
use std::collections::HashMap;

/// Register known effects of primitive functions
pub fn register_primitive_effects(symbols: &SymbolTable, effects: &mut HashMap<SymbolId, Effect>) {
    // All current primitives are pure (no yield)
    let pure_primitives = [
        // Arithmetic
        "+",
        "-",
        "*",
        "/",
        "mod",
        "abs",
        "min",
        "max",
        // Comparison
        "=",
        "<",
        ">",
        "<=",
        ">=",
        "!=",
        // Boolean
        "not",
        "and",
        "or",
        "xor",
        // List operations
        "cons",
        "first",
        "rest",
        "car",
        "cdr",
        "list",
        "length",
        "append",
        "reverse",
        "empty?",
        "nil?",
        "pair?",
        // Type predicates
        "number?",
        "string?",
        "symbol?",
        "list?",
        "fn?",
        "boolean?",
        "null?",
        "integer?",
        "float?",
        // String operations
        "string-length",
        "string-append",
        "substring",
        "string->list",
        "list->string",
        // Conversion
        "number->string",
        "string->number",
        // I/O (pure in the sense of not yielding)
        "display",
        "newline",
        "print",
        // Other
        "eq?",
        "equal?",
        "identity",
    ];

    for name in pure_primitives {
        if let Some(sym) = symbols.get(name) {
            effects.insert(sym, Effect::Pure);
        }
    }

    // Higher-order functions are polymorphic in their function argument
    let polymorphic_primitives = [
        ("map", 0),    // map's effect depends on its first arg (the function)
        ("filter", 0), // filter's effect depends on its first arg
        ("fold", 0),   // fold's effect depends on its first arg
        ("foldl", 0),
        ("foldr", 0),
        ("for-each", 0),
        ("apply", 0), // apply's effect depends on the function
    ];

    for (name, param_idx) in polymorphic_primitives {
        if let Some(sym) = symbols.get(name) {
            effects.insert(sym, Effect::Polymorphic(param_idx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_primitive_effects() {
        let mut symbols = SymbolTable::new();
        let mut effects = HashMap::new();

        // Intern some primitives
        let plus = symbols.intern("+");
        let map = symbols.intern("map");

        register_primitive_effects(&symbols, &mut effects);

        assert_eq!(effects.get(&plus), Some(&Effect::Pure));
        assert_eq!(effects.get(&map), Some(&Effect::Polymorphic(0)));
    }
}
