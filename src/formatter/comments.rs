//! Comment collection and attachment for the formatter.
//!
//! The lexer emits `Token::Comment(text)` tokens. The `SyntaxReader` skips
//! them during parsing. This module collects those comment tokens with their
//! source positions into a `CommentMap` that the formatter consults when
//! emitting output — placing comments relative to the Syntax nodes they
//! annotate.

use super::pos::{ByteOffset, ColNum, LineNum};
use crate::epoch::rules::Lexicon;
use crate::reader::{Lexer, OwnedToken, SourceLoc, Token};

/// A source comment with its position and text.
#[derive(Debug, Clone)]
pub struct SourceComment {
    /// The full comment text including the `#` prefix.
    pub text: String,
    /// Byte offset in the source where the comment starts.
    pub byte_offset: ByteOffset,
    /// 1-indexed line number.
    pub line: LineNum,
    /// 1-indexed column number.
    pub col: ColNum,
}

impl SourceComment {
    /// Build a comment from raw lexer values, wrapping each in its position
    /// newtype. The single place where untyped offsets/lines/cols enter the
    /// trivia layer.
    fn new(text: String, byte_offset: usize, line: u32, col: u32) -> Self {
        SourceComment {
            text,
            byte_offset: ByteOffset::new(byte_offset),
            line: LineNum::new(line),
            col: ColNum::new(col),
        }
    }
}

/// A map of all comments in a source file, ordered by byte offset.
#[derive(Debug, Clone)]
pub struct CommentMap {
    comments: Vec<SourceComment>,
}

impl CommentMap {
    /// Build a CommentMap from a source string under the current epoch's
    /// lexicon. Lexes the source and collects all comment tokens.
    pub fn collect(source: &str, source_name: &str) -> Result<Self, String> {
        Ok(lex_for_format(source, source_name, Lexicon::current())?.comment_map)
    }

    /// An empty comment map.
    pub fn empty() -> Self {
        CommentMap {
            comments: Vec::new(),
        }
    }

    /// Get all comments.
    pub fn comments(&self) -> &[SourceComment] {
        &self.comments
    }

    /// Drain all comments with byte offset in the range [start, end).
    /// Returns the comments and removes them from the map.
    pub fn drain_range(&mut self, start: ByteOffset, end: ByteOffset) -> Vec<SourceComment> {
        let mut result = Vec::new();
        self.comments.retain(|c| {
            if c.byte_offset >= start && c.byte_offset < end {
                result.push(c.clone());
                false
            } else {
                true
            }
        });
        result
    }

    /// Get comments that appear before a given byte offset (leading comments).
    /// These are comments whose byte offset is strictly before `offset`.
    /// Consumes the returned comments from the map.
    pub fn take_leading(&mut self, offset: ByteOffset) -> Vec<SourceComment> {
        let mut result = Vec::new();
        self.comments.retain(|c| {
            if c.byte_offset < offset {
                result.push(c.clone());
                false
            } else {
                true
            }
        });
        result
    }

    /// Get comments that appear on the same line as a given byte offset.
    /// These are trailing comments (after code on the same line).
    pub fn take_trailing(&mut self, line: LineNum) -> Vec<SourceComment> {
        let mut result = Vec::new();
        self.comments.retain(|c| {
            if c.line == line {
                result.push(c.clone());
                false
            } else {
                true
            }
        });
        result
    }

    /// Check if there are any remaining comments.
    pub fn is_empty(&self) -> bool {
        self.comments.is_empty()
    }
}

/// Result of lexing source for the formatter.
/// Contains both the regular tokens (for SyntaxReader) and the comment map.
pub struct LexedForFormat {
    pub tokens: Vec<OwnedToken>,
    pub locations: Vec<SourceLoc>,
    pub lengths: Vec<usize>,
    pub byte_offsets: Vec<usize>,
    pub comment_map: CommentMap,
}

/// Strip a shebang line from source if present.
/// Returns (stripped_source, shebang_line).
/// The shebang_line includes the trailing newline, or is empty if none.
pub fn strip_shebang(source: &str) -> (&str, &str) {
    let len = crate::reader::shebang_len(source);
    (&source[len..], &source[..len])
}

/// Lex source for formatting under `lexicon`: produces regular tokens for
/// the parser and collects comment tokens into a CommentMap.
///
/// The caller picks the lexicon from the epoch the source declares
/// (docs/impl/lexicon.md), so a file written before a token-level change
/// formats as its author wrote it.
///
/// IMPORTANT: `source` must already have its shebang stripped (if any).
/// Use `strip_shebang()` before calling this function. This ensures
/// byte offsets in the token stream agree with byte offsets in the
/// source string passed to `collect_trivia`.
pub fn lex_for_format(
    source: &str,
    source_name: &str,
    lexicon: Lexicon,
) -> Result<LexedForFormat, String> {
    let mut lexer = Lexer::with_file(source, source_name).in_lexicon(lexicon);
    let mut tokens = Vec::new();
    let mut locations = Vec::new();
    let mut lengths = Vec::new();
    let mut byte_offsets = Vec::new();
    let mut comments = Vec::new();

    loop {
        match lexer.next_token_with_loc() {
            Ok(Some(twl)) => match &twl.token {
                Token::Comment(text) => {
                    // Strip trailing newline — the lexer includes it
                    // but the formatter handles line breaks itself.
                    let trimmed = text.trim_end_matches('\n').to_string();
                    comments.push(SourceComment::new(
                        trimmed,
                        twl.byte_offset,
                        twl.loc.line as u32,
                        twl.loc.col as u32,
                    ));
                }
                _ => {
                    tokens.push(OwnedToken::from(twl.token));
                    locations.push(twl.loc);
                    lengths.push(twl.len);
                    byte_offsets.push(twl.byte_offset);
                }
            },
            Ok(None) => break,
            Err(e) => return Err(e),
        }
    }

    Ok(LexedForFormat {
        tokens,
        locations,
        lengths,
        byte_offsets,
        comment_map: CommentMap { comments },
    })
}

#[cfg(test)]
mod tests;
