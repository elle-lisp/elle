//! Comment collection and attachment for the formatter.
//!
//! The lexer emits `Token::Comment(text)` tokens. The `SyntaxReader` skips
//! them during parsing. This module collects those comment tokens with their
//! source positions into a `CommentMap` that the formatter consults when
//! emitting output — placing comments relative to the Syntax nodes they
//! annotate.

use crate::reader::{Lexer, OwnedToken, SourceLoc, Token};

/// A source comment with its position and text.
#[derive(Debug, Clone)]
pub struct SourceComment {
    /// The full comment text including the `#` prefix.
    pub text: String,
    /// Byte offset in the source where the comment starts.
    pub byte_offset: usize,
    /// 1-indexed line number.
    pub line: u32,
    /// 1-indexed column number.
    pub col: u32,
}

/// A map of all comments in a source file, ordered by byte offset.
#[derive(Debug, Clone)]
pub struct CommentMap {
    comments: Vec<SourceComment>,
}

impl CommentMap {
    /// Build a CommentMap from a source string.
    /// Lexes the source and collects all comment tokens.
    pub fn collect(source: &str, source_name: &str) -> Result<Self, String> {
        let mut lexer = Lexer::with_file(source, source_name);
        let mut comments = Vec::new();

        loop {
            match lexer.next_token_with_loc() {
                Ok(Some(twl)) => {
                    if let Token::Comment(text) = &twl.token {
                        // Strip trailing newline — the lexer includes it
                        // but the formatter handles line breaks itself.
                        let trimmed = text.trim_end_matches('\n').to_string();
                        comments.push(SourceComment {
                            text: trimmed,
                            byte_offset: twl.byte_offset,
                            line: twl.loc.line as u32,
                            col: twl.loc.col as u32,
                        });
                    }
                }
                Ok(None) => break,
                Err(e) => return Err(e),
            }
        }

        Ok(CommentMap { comments })
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
    pub fn drain_range(&mut self, start: usize, end: usize) -> Vec<SourceComment> {
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
    pub fn take_leading(&mut self, offset: usize) -> Vec<SourceComment> {
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
    pub fn take_trailing(&mut self, line: u32) -> Vec<SourceComment> {
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
    if source.starts_with("#!") {
        match source.find('\n') {
            Some(pos) => (&source[pos + 1..], &source[..pos + 1]),
            None => ("", source),
        }
    } else {
        (source, "")
    }
}

/// Lex source for formatting: produces regular tokens for the parser
/// and collects comment tokens into a CommentMap.
///
/// IMPORTANT: `source` must already have its shebang stripped (if any).
/// Use `strip_shebang()` before calling this function. This ensures
/// byte offsets in the token stream agree with byte offsets in the
/// source string passed to `collect_trivia`.
pub fn lex_for_format(source: &str, source_name: &str) -> Result<LexedForFormat, String> {
    let mut lexer = Lexer::with_file(source, source_name);
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
                    comments.push(SourceComment {
                        text: trimmed,
                        byte_offset: twl.byte_offset,
                        line: twl.loc.line as u32,
                        col: twl.loc.col as u32,
                    });
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
mod tests {
    use super::*;

    #[test]
    fn test_empty_source() {
        let map = CommentMap::collect("", "<test>").unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn test_no_comments() {
        let map = CommentMap::collect("(+ 1 2)", "<test>").unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn test_single_comment() {
        let map = CommentMap::collect("# hello", "<test>").unwrap();
        assert_eq!(map.comments().len(), 1);
        assert_eq!(map.comments()[0].text, "# hello");
        assert_eq!(map.comments()[0].line, 1);
    }

    #[test]
    fn test_multiple_comments() {
        let map = CommentMap::collect("# first\n# second\n(+ 1 2)", "<test>").unwrap();
        assert_eq!(map.comments().len(), 2);
        assert_eq!(map.comments()[0].text, "# first");
        assert_eq!(map.comments()[1].text, "# second");
    }

    #[test]
    fn test_doc_comment() {
        let map = CommentMap::collect("## doc text", "<test>").unwrap();
        assert_eq!(map.comments().len(), 1);
        assert!(map.comments()[0].text.starts_with("##"));
    }

    #[test]
    fn test_take_leading() {
        let mut map = CommentMap::collect("# before\n42 # inline\n# after", "<test>").unwrap();
        assert_eq!(map.comments().len(), 3);

        let leading = map.take_leading(10); // byte offset of "42"
        assert_eq!(leading.len(), 1);
        assert_eq!(leading[0].text, "# before");
        assert_eq!(map.comments().len(), 2);
    }

    #[test]
    fn test_take_trailing() {
        let mut map = CommentMap::collect("42 # inline\n# after", "<test>").unwrap();
        let trailing = map.take_trailing(1);
        assert_eq!(trailing.len(), 1);
        assert_eq!(trailing[0].text, "# inline");
        assert_eq!(map.comments().len(), 1);
    }

    #[test]
    fn test_lex_for_format() {
        let result = lex_for_format("# comment\n(+ 1 2)", "<test>").unwrap();
        // Regular tokens: (, +, 1, 2, )
        assert_eq!(result.tokens.len(), 5);
        // Comment map has 1 comment
        assert_eq!(result.comment_map.comments().len(), 1);
    }

    #[test]
    fn test_strip_shebang() {
        let (source, shebang) = strip_shebang("#!/usr/bin/env elle\n(+ 1 2)");
        assert_eq!(shebang, "#!/usr/bin/env elle\n");
        assert_eq!(source, "(+ 1 2)");
    }

    #[test]
    fn test_strip_no_shebang() {
        let (source, shebang) = strip_shebang("(+ 1 2)");
        assert_eq!(shebang, "");
        assert_eq!(source, "(+ 1 2)");
    }

    #[test]
    fn test_lex_for_format_shebang() {
        // lex_for_format receives already-stripped source
        let (stripped, _shebang) = strip_shebang("#!/usr/bin/env elle\n(+ 1 2)");
        let result = lex_for_format(stripped, "<test>").unwrap();
        assert_eq!(result.tokens.len(), 5);
        assert!(result.comment_map.is_empty());
    }
}
