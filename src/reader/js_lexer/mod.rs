//! JavaScript-surface-syntax tokenizer.
//!
//! Produces `JsToken` values with source locations. Numbers reuse the
//! patterns from `numeric.rs` (decimal, hex, scientific notation).

use super::scan::{CharCursor, CharIdx};
use super::token::SourceLoc;

/// Token types for the JavaScript surface syntax.
#[derive(Debug, Clone, PartialEq)]
pub enum JsToken {
    // Literals
    Int(i64),
    Float(f64),
    String(String),
    /// Template literal segments: `hello ${expr} world` produces
    /// TemplateHead("hello "), then the expression tokens, then
    /// TemplateTail(" world").  Middle segments between interpolations
    /// use TemplateMiddle.
    TemplateHead(String),
    TemplateMiddle(String),
    TemplateTail(String),
    /// A no-interpolation template: `hello world`
    TemplateNoSub(String),
    True,
    False,
    Null,
    Undefined,

    // Identifiers
    Ident(String),

    // Keywords
    Function,
    Return,
    If,
    Else,
    While,
    For,
    Of,
    In,
    Const,
    Let,
    Var,
    Break,
    Continue,
    Do,
    Switch,
    Case,
    Default,
    Typeof,
    New,
    Throw,
    Try,
    Catch,
    Finally,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    StarStar, // **
    Eq,       // ===
    Neq,      // !==
    EqLoose,  // ==
    NeqLoose, // !=
    Lt,
    Gt,
    Le,
    Ge,
    And,         // &&
    Or,          // ||
    Not,         // !
    Assign,      // =
    PlusAssign,  // +=
    MinusAssign, // -=
    StarAssign,  // *=
    SlashAssign, // /=
    Arrow,       // =>
    Dot,
    DotDotDot, // ...
    Question,  // ?
    Colon,
    PlusPlus,   // ++
    MinusMinus, // --
    Ampersand,  // & (bitwise and)
    Pipe,       // | (bitwise or)
    Caret,      // ^ (bitwise xor)
    Tilde,      // ~ (bitwise not)
    ShiftLeft,  // <<
    ShiftRight, // >>

    // Delimiters
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Semicolon,

    // Special
    #[allow(dead_code)]
    Backtick, // ` reserved for s-expr escape
    Eof,
}

/// A token with its source location and byte length.
#[derive(Debug, Clone)]
pub struct JsTokenLoc {
    pub token: JsToken,
    pub loc: SourceLoc,
    pub len: usize,
}

impl JsTokenLoc {
    /// Bundle a token with its source location and byte length. Reached through
    /// [`JsLexer::spanned`] for measured lexemes; called directly for the
    /// zero-width synthetic Eof token (here and in the parser's Eof sentinel).
    pub(super) fn new(token: JsToken, loc: SourceLoc, len: usize) -> Self {
        JsTokenLoc { token, loc, len }
    }
}

pub struct JsLexer {
    cursor: CharCursor,
    file: String,
}

impl JsLexer {
    pub fn new(input: &str, file: &str) -> Self {
        JsLexer {
            cursor: CharCursor::new(input),
            file: file.to_string(),
        }
    }

    fn loc(&self) -> SourceLoc {
        SourceLoc::new(&self.file, self.cursor.line(), self.cursor.col())
    }

    /// Bundle `token` with the span from `start` to the cursor's current
    /// position, deriving `len` via the cursor in one place.
    fn spanned(&self, token: JsToken, loc: SourceLoc, start: CharIdx) -> JsTokenLoc {
        JsTokenLoc::new(token, loc, self.cursor.offset_from(start))
    }

    fn peek(&self) -> Option<char> {
        self.cursor.peek()
    }

    fn peek2(&self) -> Option<char> {
        self.cursor.nth(1)
    }

    fn peek3(&self) -> Option<char> {
        self.cursor.nth(2)
    }

    fn advance(&mut self) -> Option<char> {
        self.cursor.advance()
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn skip_line_comment(&mut self) {
        while let Some(c) = self.advance() {
            if c == '\n' {
                break;
            }
        }
    }

    fn skip_block_comment(&mut self) -> Result<(), String> {
        let start_line = self.cursor.line();
        loop {
            match self.advance() {
                None => {
                    return Err(format!(
                        "{}:{}:{}: unterminated block comment starting at line {}",
                        self.file,
                        self.cursor.line(),
                        self.cursor.col(),
                        start_line
                    ));
                }
                Some('*') if self.peek() == Some('/') => {
                    self.advance();
                    return Ok(());
                }
                _ => {}
            }
        }
    }

    /// Tokenize the entire input, returning all tokens with locations.
    /// Template literals with interpolation produce multiple tokens:
    /// TemplateHead, expression tokens, TemplateMiddle/TemplateTail.
    pub fn tokenize(&mut self) -> Result<Vec<JsTokenLoc>, String> {
        let mut tokens = Vec::new();
        let mut template_depth: u32 = 0; // track nested template brace depth

        loop {
            self.skip_whitespace();
            let loc = self.loc();
            let start_pos = self.cursor.pos();

            let c = match self.peek() {
                None => {
                    tokens.push(JsTokenLoc::new(JsToken::Eof, loc, 0));
                    return Ok(tokens);
                }
                Some(c) => c,
            };

            // Handle closing brace inside template interpolation
            if c == '}' && template_depth > 0 {
                self.advance();
                template_depth -= 1;
                // Continue reading the template literal
                let tpl_tokens = self.continue_template()?;
                for tt in tpl_tokens {
                    let is_middle = matches!(&tt, JsToken::TemplateMiddle(_));
                    tokens.push(self.spanned(tt, loc.clone(), start_pos));
                    if is_middle {
                        template_depth += 1;
                    }
                }
                continue;
            }

            let token = match c {
                // Comments
                '/' if self.peek2() == Some('/') => {
                    self.advance();
                    self.advance();
                    self.skip_line_comment();
                    continue;
                }
                '/' if self.peek2() == Some('*') => {
                    self.advance();
                    self.advance();
                    self.skip_block_comment()?;
                    continue;
                }

                // String literals
                '"' | '\'' => {
                    let s = self.read_string(c)?;
                    JsToken::String(s)
                }

                // Template literals
                '`' => {
                    let tpl_tokens = self.read_template()?;
                    for tt in tpl_tokens {
                        let is_head = matches!(&tt, JsToken::TemplateHead(_));
                        tokens.push(self.spanned(tt, loc.clone(), start_pos));
                        if is_head {
                            template_depth += 1;
                        }
                    }
                    continue;
                }

                // Numbers
                '0'..='9' => self.read_number()?,

                // Identifiers and keywords
                c if c.is_alphabetic() || c == '_' || c == '$' => {
                    let name = self.read_ident();
                    self.keyword_or_ident(&name)
                }

                // Three-char operators
                '=' if self.peek2() == Some('=') && self.peek3() == Some('=') => {
                    self.advance();
                    self.advance();
                    self.advance();
                    JsToken::Eq
                }
                '!' if self.peek2() == Some('=') && self.peek3() == Some('=') => {
                    self.advance();
                    self.advance();
                    self.advance();
                    JsToken::Neq
                }
                '.' if self.peek2() == Some('.') && self.peek3() == Some('.') => {
                    self.advance();
                    self.advance();
                    self.advance();
                    JsToken::DotDotDot
                }
                '*' if self.peek2() == Some('*') => {
                    self.advance();
                    self.advance();
                    JsToken::StarStar
                }

                // Two-char operators
                '=' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    JsToken::EqLoose
                }
                '!' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    JsToken::NeqLoose
                }
                '=' if self.peek2() == Some('>') => {
                    self.advance();
                    self.advance();
                    JsToken::Arrow
                }
                '<' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    JsToken::Le
                }
                '>' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    JsToken::Ge
                }
                '<' if self.peek2() == Some('<') => {
                    self.advance();
                    self.advance();
                    JsToken::ShiftLeft
                }
                '>' if self.peek2() == Some('>') => {
                    self.advance();
                    self.advance();
                    JsToken::ShiftRight
                }
                '&' if self.peek2() == Some('&') => {
                    self.advance();
                    self.advance();
                    JsToken::And
                }
                '|' if self.peek2() == Some('|') => {
                    self.advance();
                    self.advance();
                    JsToken::Or
                }
                '+' if self.peek2() == Some('+') => {
                    self.advance();
                    self.advance();
                    JsToken::PlusPlus
                }
                '-' if self.peek2() == Some('-') => {
                    self.advance();
                    self.advance();
                    JsToken::MinusMinus
                }
                '+' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    JsToken::PlusAssign
                }
                '-' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    JsToken::MinusAssign
                }
                '*' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    JsToken::StarAssign
                }
                '/' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    JsToken::SlashAssign
                }

                // Single-char operators and delimiters
                '+' => {
                    self.advance();
                    JsToken::Plus
                }
                '-' => {
                    self.advance();
                    JsToken::Minus
                }
                '*' => {
                    self.advance();
                    JsToken::Star
                }
                '/' => {
                    self.advance();
                    JsToken::Slash
                }
                '%' => {
                    self.advance();
                    JsToken::Percent
                }
                '<' => {
                    self.advance();
                    JsToken::Lt
                }
                '>' => {
                    self.advance();
                    JsToken::Gt
                }
                '=' => {
                    self.advance();
                    JsToken::Assign
                }
                '!' => {
                    self.advance();
                    JsToken::Not
                }
                '?' => {
                    self.advance();
                    JsToken::Question
                }
                '.' => {
                    self.advance();
                    JsToken::Dot
                }
                ':' => {
                    self.advance();
                    JsToken::Colon
                }
                '&' => {
                    self.advance();
                    JsToken::Ampersand
                }
                '|' => {
                    self.advance();
                    JsToken::Pipe
                }
                '^' => {
                    self.advance();
                    JsToken::Caret
                }
                '~' => {
                    self.advance();
                    JsToken::Tilde
                }
                '(' => {
                    self.advance();
                    JsToken::LParen
                }
                ')' => {
                    self.advance();
                    JsToken::RParen
                }
                '[' => {
                    self.advance();
                    JsToken::LBracket
                }
                ']' => {
                    self.advance();
                    JsToken::RBracket
                }
                '{' => {
                    self.advance();
                    JsToken::LBrace
                }
                '}' => {
                    self.advance();
                    JsToken::RBrace
                }
                ',' => {
                    self.advance();
                    JsToken::Comma
                }
                ';' => {
                    self.advance();
                    JsToken::Semicolon
                }

                _ => {
                    return Err(format!(
                        "{}:{}:{}: unexpected character '{}'",
                        self.file,
                        self.cursor.line(),
                        self.cursor.col(),
                        c
                    ));
                }
            };

            tokens.push(self.spanned(token, loc, start_pos));
        }
    }
}

mod literals;
#[cfg(test)]
mod tests;
