pub mod cursor;
mod js_lexer;
mod js_parser;
mod lexer;
mod lua_lexer;
mod lua_parser;
mod nav;
mod numeric;
mod parser;
mod py_lexer;
mod py_parser;
pub mod scan;
mod synbuild;
mod syntax;
mod token;

// Re-export public API
pub use lexer::Lexer;
pub use parser::Reader;
pub use syntax::SyntaxReader;
pub use token::{OwnedToken, SourceLoc, Token, TokenWithLoc, UNKNOWN_FILE};

use crate::symbol::SymbolTable;
use crate::syntax::Syntax;
use crate::value::Value;

/// Main public entry point for reading Lisp code from a string. The result is
/// born in a fresh region on `heap` (the caller's instance heap — an embedder
/// passes `runtime.heap()`), escaping value-based to the caller.
pub fn read_str(
    input: &str,
    heap: &mut crate::value::fiberheap::FiberHeap,
    symbols: &mut SymbolTable,
) -> Result<Value, String> {
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
    // The read's allocation capability over a fresh region on the caller's heap
    // (docs/impl/region/ctx.md — explicit region). The returned Value lives in
    // this region and escapes to the caller value-based; the caller owns its
    // lifetime (this entry point runs no per-read teardown).
    let region = heap.new_runtime_region();
    let mut ctx = crate::primitives::ctx::Alloc::with_region(region, heap);
    reader.read(&mut ctx, symbols)
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

/// Parse source, dispatching by file extension:
/// `.lua` → Lua reader, `.js` → JavaScript reader, `.py` → Python reader,
/// `.md` → markdown code-block extraction, anything else → s-expressions.
pub fn read_syntax_all_for(input: &str, source_name: &str) -> Result<Vec<Syntax>, String> {
    if source_name.ends_with(".lua") {
        lua_parser::parse_lua_file(input, source_name)
    } else if source_name.ends_with(".js") {
        js_parser::parse_js_file(input, source_name)
    } else if source_name.ends_with(".py") {
        py_parser::parse_py_file(input, source_name)
    } else if source_name.ends_with(".md") {
        let stripped = strip_markdown(input);
        read_syntax_all(&stripped, source_name)
    } else {
        read_syntax_all(input, source_name)
    }
}

// Tests migrated to tests/elle/reader.lisp
