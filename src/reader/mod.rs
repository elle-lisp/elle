mod js_lexer;
mod js_parser;
mod lexer;
mod lua_lexer;
mod lua_parser;
mod numeric;
mod parser;
mod syntax;
mod token;

// Re-export public API
pub use lexer::Lexer;
pub use parser::Reader;
pub use syntax::SyntaxReader;
pub use token::{OwnedToken, SourceLoc, Token, TokenWithLoc};

use crate::symbol::SymbolTable;
use crate::syntax::Syntax;
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
    let mut locations = Vec::new();

    while let Some(token_with_loc) = lexer.next_token_with_loc()? {
        tokens.push(OwnedToken::from(token_with_loc.token));
        locations.push(token_with_loc.loc);
    }

    if tokens.is_empty() {
        return Err("No input".to_string());
    }

    let mut reader = Reader::with_locations(tokens, locations);
    reader.read(symbols)
}

/// Tokenized source ready for the syntax parser.
struct LexedTokens {
    tokens: Vec<OwnedToken>,
    locations: Vec<SourceLoc>,
    lengths: Vec<usize>,
    byte_offsets: Vec<usize>,
}

/// Lex source into tokens with source locations and byte offsets.
fn lex_all(input: &str, source_name: &str) -> Result<LexedTokens, String> {
    // Strip shebang if present
    let input_owned = if input.starts_with("#!") {
        input.lines().skip(1).collect::<Vec<_>>().join("\n")
    } else {
        input.to_string()
    };

    let mut lexer = Lexer::with_file(&input_owned, source_name);
    let mut tokens = Vec::new();
    let mut locations = Vec::new();
    let mut lengths = Vec::new();
    let mut byte_offsets = Vec::new();

    while let Some(twl) = lexer.next_token_with_loc()? {
        tokens.push(OwnedToken::from(twl.token));
        locations.push(twl.loc);
        lengths.push(twl.len);
        byte_offsets.push(twl.byte_offset);
    }

    Ok(LexedTokens {
        tokens,
        locations,
        lengths,
        byte_offsets,
    })
}

/// Parse source code into a Syntax tree
pub fn read_syntax(input: &str, source_name: &str) -> Result<Syntax, String> {
    let lex = lex_all(input, source_name)?;

    if lex.tokens.is_empty() {
        return Err("No input".to_string());
    }

    let mut parser =
        SyntaxReader::with_byte_offsets(lex.tokens, lex.locations, lex.lengths, lex.byte_offsets);
    let result = parser.read()?;

    if let Some(err) = parser.check_exhausted() {
        return Err(err);
    }

    Ok(result)
}

/// Parse source code into multiple Syntax trees
pub fn read_syntax_all(input: &str, source_name: &str) -> Result<Vec<Syntax>, String> {
    let lex = lex_all(input, source_name)?;

    if lex.tokens.is_empty() {
        return Ok(Vec::new());
    }

    let mut parser =
        SyntaxReader::with_byte_offsets(lex.tokens, lex.locations, lex.lengths, lex.byte_offsets);
    parser.read_all()
}

/// Strip markdown prose, keeping only ```lisp / ```elle fenced code blocks.
/// Non-code lines become empty (preserving line numbers for error reporting).
pub fn strip_markdown(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut in_code = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if !in_code && (trimmed == "```lisp" || trimmed == "```elle") {
            in_code = true;
            out.push('\n');
        } else if in_code && trimmed.starts_with("```") {
            in_code = false;
            out.push('\n');
        } else if in_code {
            out.push_str(line);
            out.push('\n');
        } else {
            out.push('\n');
        }
    }
    out
}

/// Parse source, dispatching to the Lua reader for `.lua` files,
/// the JavaScript reader for `.js` files,
/// and stripping markdown for `.md` files.
pub fn read_syntax_all_for(input: &str, source_name: &str) -> Result<Vec<Syntax>, String> {
    if source_name.ends_with(".lua") {
        lua_parser::parse_lua_file(input, source_name)
    } else if source_name.ends_with(".js") {
        js_parser::parse_js_file(input, source_name)
    } else if source_name.ends_with(".md") {
        let stripped = strip_markdown(input);
        read_syntax_all(&stripped, source_name)
    } else {
        read_syntax_all(input, source_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_read_number() {
        let mut symbols = SymbolTable::new();
        assert_eq!(read_str("42", &mut symbols).unwrap(), Value::int(42));
        assert_eq!(read_str("3.14", &mut symbols).unwrap(), Value::float(3.14));
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
