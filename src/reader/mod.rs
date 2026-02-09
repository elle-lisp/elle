mod lexer;
mod parser;
mod token;

// Re-export public API
pub use lexer::Lexer;
pub use parser::Reader;
pub use token::{OwnedToken, SourceLoc, Token, TokenWithLoc};

use crate::symbol::SymbolTable;
use crate::value::Value;

/// Main public entry point for reading Lisp code from a string
pub fn read_str(input: &str, symbols: &mut SymbolTable) -> Result<Value, String> {
    // Strip shebang if present (e.g., #!/usr/bin/env elle)
    let input_owned = if input.starts_with("#!") {
        // Find the end of the first line and skip it
        input.lines().skip(1).collect::<Vec<_>>().join("\n")
    } else {
        input.to_string()
    };

    let mut lexer = Lexer::new(&input_owned);
    let mut tokens = Vec::new();

    while let Some(token) = lexer.next_token()? {
        tokens.push(OwnedToken::from(token));
    }

    if tokens.is_empty() {
        return Err("No input".to_string());
    }

    let mut reader = Reader::new(tokens);
    reader.read(symbols)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_read_number() {
        let mut symbols = SymbolTable::new();
        assert_eq!(read_str("42", &mut symbols).unwrap(), Value::Int(42));
        assert_eq!(read_str("3.14", &mut symbols).unwrap(), Value::Float(3.14));
    }

    #[test]
    fn test_read_list() {
        let mut symbols = SymbolTable::new();
        let result = read_str("(1 2 3)", &mut symbols).unwrap();
        assert!(result.is_list());
        let vec = result.list_to_vec().unwrap();
        assert_eq!(vec.len(), 3);
    }

    #[test]
    fn test_read_quote() {
        let mut symbols = SymbolTable::new();
        let result = read_str("'foo", &mut symbols).unwrap();
        let vec = result.list_to_vec().unwrap();
        assert_eq!(vec.len(), 2);
    }
}
