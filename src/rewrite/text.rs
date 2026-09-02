//! The text a rewrite reads, paired with the rules that tokenize it.

use crate::epoch::rules::Lexicon;
use crate::reader::{Lexer, Token, TokenWithLoc};

/// Source text, its name, and the lexicon that tokenizes it.
///
/// Every pass in the rewriter lexes the same bytes, and they must all lex
/// them the same way: a file written before a token-level change tokenizes
/// under its own epoch's rules, not the current ones (docs/impl/lexicon.md).
/// Passing the three together is what keeps a pass from reaching for
/// `Lexer::new` and getting the current epoch by habit.
#[derive(Clone, Copy)]
pub(crate) struct SourceText<'a> {
    /// The bytes, exactly as the file holds them. Edits index into these.
    pub text: &'a str,
    /// The file name, for error messages.
    pub name: &'a str,
    /// The rules these bytes were written under.
    pub lexicon: Lexicon,
}

impl<'a> SourceText<'a> {
    pub(crate) fn new(text: &'a str, name: &'a str, lexicon: Lexicon) -> Self {
        SourceText {
            text,
            name,
            lexicon,
        }
    }

    /// Whether `token` is part of the shebang line. That line is the
    /// operating system's, not Elle's, however a lexicon happens to
    /// tokenize it — a `#`-commenting one makes it a comment, another
    /// makes it symbols. No rewrite may touch either reading.
    pub(crate) fn in_shebang(&self, token: &TokenWithLoc<'_>) -> bool {
        token.byte_offset < crate::reader::shebang_len(self.text)
    }

    /// Every token, comments included, with its span.
    pub(crate) fn tokens(&self) -> Result<Vec<TokenWithLoc<'a>>, String> {
        let mut lexer = Lexer::with_file(self.text, self.name).in_lexicon(self.lexicon);
        let mut tokens = Vec::new();
        while let Some(token) = lexer.next_token_with_loc()? {
            tokens.push(token);
        }
        Ok(tokens)
    }

    /// Every token but the comments, as `(token, byte_offset, len)`.
    ///
    /// The passes built on this match forms by token position and count, so
    /// a comment in the stream would shift every index after it.
    pub(crate) fn code_tokens(&self) -> Result<Vec<(Token<'a>, usize, usize)>, String> {
        Ok(self
            .tokens()?
            .into_iter()
            .filter(|t| !matches!(t.token, Token::Comment(_)))
            .map(|t| (t.token, t.byte_offset, t.len))
            .collect())
    }
}
