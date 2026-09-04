//! Tests for SyntaxReader

use super::*;
use crate::reader::Lexer;

fn lex_and_parse(input: &str) -> Result<Syntax, String> {
    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    let mut locations = Vec::new();
    let mut lengths = Vec::new();

    while let Some(token_with_loc) = lexer.next_token_with_loc()? {
        tokens.push(OwnedToken::from(token_with_loc.token));
        locations.push(token_with_loc.loc);
        lengths.push(token_with_loc.len);
    }

    let mut reader = SyntaxReader::new(tokens, locations, lengths, crate::syntax::thread_arena());
    reader.read()
}

fn lex_and_parse_all(input: &str) -> Result<Vec<Syntax>, String> {
    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    let mut locations = Vec::new();
    let mut lengths = Vec::new();

    while let Some(token_with_loc) = lexer.next_token_with_loc()? {
        tokens.push(OwnedToken::from(token_with_loc.token));
        locations.push(token_with_loc.loc);
        lengths.push(token_with_loc.len);
    }

    let mut reader = SyntaxReader::new(tokens, locations, lengths, crate::syntax::thread_arena());
    reader.read_all()
}

/// Lex `input` into the four parallel columns the lexer produces.
fn lex_columns(input: &str) -> (Vec<OwnedToken>, Vec<SourceLoc>, Vec<usize>, Vec<usize>) {
    let mut lexer = Lexer::new(input);
    let (mut tokens, mut locs, mut lens, mut offs) = (vec![], vec![], vec![], vec![]);
    while let Some(twl) = lexer.next_token_with_loc().unwrap() {
        tokens.push(OwnedToken::from(twl.token));
        locs.push(twl.loc);
        lens.push(twl.len);
        offs.push(twl.byte_offset);
    }
    (tokens, locs, lens, offs)
}

mod lexing;
mod parsing;
