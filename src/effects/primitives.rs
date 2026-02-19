//! Known effects of built-in primitives

use super::Effect;
use crate::symbol::SymbolTable;
use crate::value::SymbolId;
use std::collections::HashMap;

/// Register known effects of primitive functions.
/// This interns the primitive names if they aren't already interned.
pub fn register_primitive_effects(
    symbols: &mut SymbolTable,
    effects: &mut HashMap<SymbolId, Effect>,
) {
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
        let sym = symbols.intern(name);
        effects.insert(sym, Effect::Pure);
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
        let sym = symbols.intern(name);
        effects.insert(sym, Effect::polymorphic(param_idx));
    }
}

/// Build the primitive effects map without modifying the symbol table.
/// Only includes effects for primitives that are already interned.
pub fn get_primitive_effects(symbols: &SymbolTable) -> HashMap<SymbolId, Effect> {
    let mut effects = HashMap::new();

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
            effects.insert(sym, Effect::polymorphic(param_idx));
        }
    }

    effects
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

        register_primitive_effects(&mut symbols, &mut effects);

        assert_eq!(effects.get(&plus), Some(&Effect::Pure));
        assert_eq!(effects.get(&map), Some(&Effect::polymorphic(0)));
    }

    #[test]
    fn test_get_primitive_effects() {
        let mut symbols = SymbolTable::new();

        // Intern some primitives first
        let plus = symbols.intern("+");
        let map = symbols.intern("map");

        let effects = get_primitive_effects(&symbols);

        assert_eq!(effects.get(&plus), Some(&Effect::Pure));
        assert_eq!(effects.get(&map), Some(&Effect::polymorphic(0)));
    }
}
