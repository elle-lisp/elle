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

use std::borrow::Cow;

use crate::epoch::rules::Lexicon;
use crate::symbol::SymbolTable;
use crate::syntax::Syntax;
use crate::value::Value;

/// The byte length of a leading shebang line, including its newline.
///
/// A shebang (`#!/usr/bin/env elle`) belongs to the operating system, not to
/// Elle, so it never reaches the lexer. Everything that translates between
/// original-source offsets and lexer offsets — the reader, the prescan, the
/// epoch detector, the rewriter, the formatter — measures the gap with this,
/// so they cannot disagree about where Elle's text begins.
pub fn shebang_len(input: &str) -> usize {
    if input.starts_with("#!") {
        input.find('\n').map(|i| i + 1).unwrap_or(input.len())
    } else {
        0
    }
}

/// The source the lexer sees: everything after a shebang line.
fn strip_shebang(input: &str) -> &str {
    &input[shebang_len(input)..]
}

/// The lexicon that tokenizes `input` (docs/impl/lexicon.md).
///
/// Pass the original source: the prescan skips a shebang itself, and it
/// must read the declaration before any rule the declaration selects has
/// been applied to the text.
pub(crate) fn lexicon_for(input: &str) -> Result<Lexicon, String> {
    Ok(Lexicon::for_epoch(crate::epoch::prescan_epoch(input)?))
}

/// Main public entry point for reading Lisp code from a string. The result is
/// born in a fresh region on `heap` (the caller's instance heap — an embedder
/// passes `runtime.heap()`), escaping value-based to the caller.
pub fn read_str(
    input: &str,
    heap: &mut crate::value::fiberheap::FiberHeap,
    symbols: &mut SymbolTable,
) -> Result<Value, String> {
    let lexicon = lexicon_for(input)?;
    let mut lexer = Lexer::new(strip_shebang(input)).in_lexicon(lexicon);
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

/// Lex source into tokens with source locations and byte offsets, under the
/// lexicon its own epoch declaration selects.
fn lex_all(input: &str, source_name: &str) -> Result<LexedTokens, String> {
    lex_all_under(input, source_name, lexicon_for(input)?)
}

/// Lex source into tokens with source locations and byte offsets, under an
/// explicit lexicon.
fn lex_all_under(input: &str, source_name: &str, lexicon: Lexicon) -> Result<LexedTokens, String> {
    let mut lexer = Lexer::with_file(strip_shebang(input), source_name).in_lexicon(lexicon);
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

/// Parse source code into a Syntax tree, born in `arena`
pub fn read_syntax(
    arena: crate::syntax::SyntaxArena,
    input: &str,
    source_name: &str,
) -> Result<Syntax, String> {
    let lex = lex_all(input, source_name)?;

    if lex.tokens.is_empty() {
        return Err("No input".to_string());
    }

    let mut parser = SyntaxReader::with_byte_offsets(
        lex.tokens,
        lex.locations,
        lex.lengths,
        lex.byte_offsets,
        arena,
    );
    let result = parser.read()?;

    if let Some(err) = parser.check_exhausted() {
        return Err(err);
    }

    Ok(result)
}

/// Parse source code into multiple Syntax trees, born in `arena`
pub fn read_syntax_all(
    arena: crate::syntax::SyntaxArena,
    input: &str,
    source_name: &str,
) -> Result<Vec<Syntax>, String> {
    parse_all(lex_all(input, source_name)?, arena)
}

/// Parse source code into multiple Syntax trees under the current epoch's
/// lexicon, whatever the text declares (docs/impl/lexicon.md).
///
/// The REPL reads this way. A declaration pasted at the prompt is a form in
/// the session, not a choice of lexer for the prompt that follows it.
pub fn read_syntax_all_current(
    arena: crate::syntax::SyntaxArena,
    input: &str,
    source_name: &str,
) -> Result<Vec<Syntax>, String> {
    parse_all(
        lex_all_under(input, source_name, Lexicon::current())?,
        arena,
    )
}

/// Parse a lexed token stream into every form it holds.
fn parse_all(lex: LexedTokens, arena: crate::syntax::SyntaxArena) -> Result<Vec<Syntax>, String> {
    if lex.tokens.is_empty() {
        return Ok(Vec::new());
    }

    let mut parser = SyntaxReader::with_byte_offsets(
        lex.tokens,
        lex.locations,
        lex.lengths,
        lex.byte_offsets,
        arena,
    );
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

/// The s-expression text inside `input`: a literate document contributes
/// only its fenced code, everything else contributes itself. One home for
/// the rule, so the epoch prescan reads exactly the bytes the lexer will.
fn sexp_text<'a>(input: &'a str, source_name: &str) -> Cow<'a, str> {
    if source_name.ends_with(".md") {
        Cow::Owned(strip_markdown(input))
    } else {
        Cow::Borrowed(input)
    }
}

/// The epoch whose lexicon tokenizes `input` under `source_name` — the same
/// choice [`read_syntax_all_for`] makes (docs/impl/lexicon.md).
///
/// The pipeline compares this against the declaration in the parsed tree.
/// The other syntax modes have no lexicon to select, so this answers for
/// them as though they were s-expressions; the comparison never reaches
/// them, because their trees carry no `(elle/epoch N)` first form.
pub fn prescanned_epoch_for(input: &str, source_name: &str) -> Result<u64, String> {
    crate::epoch::prescan_epoch(&sexp_text(input, source_name))
}

/// Parse source, dispatching by file extension:
/// `.lua` → Lua reader, `.js` → JavaScript reader, `.py` → Python reader,
/// `.md` → markdown code-block extraction, anything else → s-expressions.
pub fn read_syntax_all_for(
    arena: crate::syntax::SyntaxArena,
    input: &str,
    source_name: &str,
) -> Result<Vec<Syntax>, String> {
    if source_name.ends_with(".lua") {
        lua_parser::parse_lua_file(arena, input, source_name)
    } else if source_name.ends_with(".js") {
        js_parser::parse_js_file(arena, input, source_name)
    } else if source_name.ends_with(".py") {
        py_parser::parse_py_file(arena, input, source_name)
    } else {
        read_syntax_all(arena, &sexp_text(input, source_name), source_name)
    }
}

// Tests migrated to tests/elle/reader.lisp

#[cfg(test)]
mod tests;
