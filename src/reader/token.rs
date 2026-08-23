use std::fmt;

/// Sentinel file name for a [`SourceLoc`] whose origin is unknown.
///
/// One source of truth: the constructors that default the file
/// ([`SourceLoc::from_line_col`], [`SourceLoc::start`]) and the predicate that
/// tests for it ([`SourceLoc::is_unknown`]) must agree, so they all reference
/// this const rather than re-spelling the literal (which previously appeared in
/// three places here plus the lexer, free to drift).
pub const UNKNOWN_FILE: &str = "<unknown>";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceLoc {
    pub file: String,
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for SourceLoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.position())
    }
}

impl SourceLoc {
    pub fn new(file: impl Into<String>, line: usize, col: usize) -> Self {
        SourceLoc {
            file: file.into(),
            line,
            col,
        }
    }

    /// Create a location from line and column (file set to unknown)
    pub fn from_line_col(line: usize, col: usize) -> Self {
        SourceLoc {
            file: UNKNOWN_FILE.to_string(),
            line,
            col,
        }
    }

    /// Create a location at the beginning of a file
    pub fn start() -> Self {
        SourceLoc {
            file: UNKNOWN_FILE.to_string(),
            line: 1,
            col: 1,
        }
    }

    /// Get position as "file:line:col" string
    pub fn position(&self) -> String {
        format!("{}:{}:{}", self.file, self.line, self.col)
    }

    /// Check if this is an unknown location
    pub fn is_unknown(&self) -> bool {
        self.file == UNKNOWN_FILE
    }

    /// Create a copy with a different file name
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = file.into();
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TokenWithLoc<'a> {
    pub token: Token<'a>,
    pub loc: SourceLoc,
    /// Source byte length of the token. Computed by the lexer so the syntax
    /// parser can build accurate spans without per-token-type heuristics, and
    /// without carrying a width on individual variants (e.g. `Bool(bool,
    /// usize)`).
    pub len: usize,
    /// Byte offset in source where this token starts.
    pub byte_offset: usize,
}

impl<'a> TokenWithLoc<'a> {
    /// Bundle a token with its source span.
    ///
    /// The lexer reaches this through its private `spanned` helper, which
    /// derives `len`/`byte_offset` from the cursor in one place; this is the
    /// plain field constructor it builds on, so every token-spanning site
    /// shares a single field-construction point.
    pub fn new(token: Token<'a>, loc: SourceLoc, len: usize, byte_offset: usize) -> Self {
        TokenWithLoc {
            token,
            loc,
            len,
            byte_offset,
        }
    }
}

/// Declares the lexer's token set exactly once, generating from it:
///
/// * [`Token`] — the borrowed form yielded by the lexer (string payloads
///   borrow from the source buffer as `&'a str`),
/// * [`OwnedToken`] — the owned form the [`Reader`](super::Reader) stores
///   (string payloads are `String`), and
/// * `From<Token> for OwnedToken` — the borrow→owned conversion.
///
/// These three — two enums and the conversion match — are generated from one
/// declaration: a variant is declared once and all three stay in sync by
/// construction, so there is no parallel hand-written list to edit in lockstep
/// (where a missed edit could fail to compile confusingly, or — worse for the
/// conversion — silently mistranslate).
///
/// Variants fall into three groups by how they cross the borrow→owned boundary:
/// `units` carry no payload, `borrowed` carry a string that borrows in `Token`
/// and is owned in `OwnedToken`, and `owned` carry a payload identical in both.
macro_rules! declare_tokens {
    (
        units { $($unit:ident),* $(,)? }
        borrowed { $($bname:ident),* $(,)? }
        owned { $($oname:ident($oty:ty)),* $(,)? }
    ) => {
        #[derive(Debug, Clone, PartialEq)]
        pub enum Token<'a> {
            $($unit,)*
            $($bname(&'a str),)*
            $($oname($oty),)*
        }

        /// Owned token variant for storage in Reader.
        #[derive(Debug, Clone, PartialEq)]
        pub enum OwnedToken {
            $($unit,)*
            $($bname(String),)*
            $($oname($oty),)*
        }

        impl<'a> From<Token<'a>> for OwnedToken {
            fn from(token: Token<'a>) -> Self {
                match token {
                    $(Token::$unit => OwnedToken::$unit,)*
                    $(Token::$bname(s) => OwnedToken::$bname(s.to_string()),)*
                    $(Token::$oname(v) => OwnedToken::$oname(v),)*
                }
            }
        }
    };
}

declare_tokens! {
    units {
        LeftParen, RightParen,
        LeftBracket, RightBracket,
        LeftBrace, RightBrace,
        Quote, Quasiquote, Unquote, UnquoteSplicing, Splice,
        ListSugar,      // @ for list sugar
        Pipe,           // | delimiter for set literals
        AtPipe,         // @| for mutable set literals
        BytesBracket,   // b[ for bytes literals
        AtBytesBracket, // @b[ for mutable @bytes literals
        Nil,
    }
    borrowed { Symbol, Keyword }
    owned {
        Integer(i64),
        Float(f64),
        String(String),
        Bool(bool),
        Comment(String),
    }
}

#[cfg(test)]
mod tests;
