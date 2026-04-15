//! JavaScript-surface-syntax tokenizer.
//!
//! Produces `JsToken` values with source locations. Numbers reuse the
//! patterns from `numeric.rs` (decimal, hex, scientific notation).

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

pub struct JsLexer {
    input: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    file: String,
}

impl JsLexer {
    pub fn new(input: &str, file: &str) -> Self {
        JsLexer {
            input: input.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            file: file.to_string(),
        }
    }

    fn loc(&self) -> SourceLoc {
        SourceLoc::new(&self.file, self.line, self.col)
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<char> {
        self.input.get(self.pos + 1).copied()
    }

    fn peek3(&self) -> Option<char> {
        self.input.get(self.pos + 2).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
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
        let start_line = self.line;
        loop {
            match self.advance() {
                None => {
                    return Err(format!(
                        "{}:{}:{}: unterminated block comment starting at line {}",
                        self.file, self.line, self.col, start_line
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

    fn read_string(&mut self, quote: char) -> Result<String, String> {
        let start_loc = self.loc();
        self.advance(); // skip opening quote
        let mut s = std::string::String::new();
        loop {
            match self.advance() {
                None => {
                    return Err(format!("{}: unterminated string", start_loc.position()));
                }
                Some('\\') => match self.advance() {
                    None => {
                        return Err(format!(
                            "{}: unterminated string escape",
                            start_loc.position()
                        ));
                    }
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some('\\') => s.push('\\'),
                    Some('\'') => s.push('\''),
                    Some('"') => s.push('"'),
                    Some('0') => s.push('\0'),
                    Some('`') => s.push('`'),
                    Some('$') => s.push('$'),
                    Some('x') => {
                        let mut hex = String::new();
                        for _ in 0..2 {
                            match self.advance() {
                                Some(c) if c.is_ascii_hexdigit() => hex.push(c),
                                _ => {
                                    return Err(format!(
                                        "{}: invalid \\x escape",
                                        start_loc.position()
                                    ))
                                }
                            }
                        }
                        let val = u8::from_str_radix(&hex, 16).unwrap();
                        s.push(val as char);
                    }
                    Some('u') => {
                        // \u{NNNN} or \uNNNN
                        if self.peek() == Some('{') {
                            self.advance();
                            let mut hex = String::new();
                            while self.peek().is_some_and(|c| c != '}') {
                                hex.push(self.advance().unwrap());
                            }
                            self.advance(); // }
                            let val = u32::from_str_radix(&hex, 16).map_err(|e| {
                                format!("{}: invalid \\u escape: {}", start_loc.position(), e)
                            })?;
                            s.push(char::from_u32(val).ok_or_else(|| {
                                format!("{}: invalid unicode codepoint", start_loc.position())
                            })?);
                        } else {
                            let mut hex = String::new();
                            for _ in 0..4 {
                                match self.advance() {
                                    Some(c) if c.is_ascii_hexdigit() => hex.push(c),
                                    _ => {
                                        return Err(format!(
                                            "{}: invalid \\u escape",
                                            start_loc.position()
                                        ))
                                    }
                                }
                            }
                            let val = u32::from_str_radix(&hex, 16).unwrap();
                            s.push(char::from_u32(val).unwrap_or('\u{FFFD}'));
                        }
                    }
                    Some(c) => {
                        return Err(format!(
                            "{}: unknown escape sequence \\{}",
                            start_loc.position(),
                            c
                        ));
                    }
                },
                Some(c) if c == quote => return Ok(s),
                Some(c) => s.push(c),
            }
        }
    }

    /// Read a template literal after the opening backtick.
    /// Returns one or more tokens for the template segments.
    fn read_template(&mut self) -> Result<Vec<JsToken>, String> {
        let start_loc = self.loc();
        self.advance(); // skip opening backtick
        let mut tokens = Vec::new();
        let mut s = String::new();
        let is_first = true;

        loop {
            match self.peek() {
                None => {
                    return Err(format!(
                        "{}: unterminated template literal",
                        start_loc.position()
                    ));
                }
                Some('\\') => {
                    self.advance();
                    match self.advance() {
                        Some('n') => s.push('\n'),
                        Some('t') => s.push('\t'),
                        Some('r') => s.push('\r'),
                        Some('\\') => s.push('\\'),
                        Some('`') => s.push('`'),
                        Some('$') => s.push('$'),
                        Some(c) => {
                            s.push('\\');
                            s.push(c);
                        }
                        None => {
                            return Err(format!(
                                "{}: unterminated template escape",
                                start_loc.position()
                            ));
                        }
                    }
                }
                Some('$') if self.peek2() == Some('{') => {
                    self.advance(); // $
                    self.advance(); // {
                    if is_first {
                        tokens.push(JsToken::TemplateHead(std::mem::take(&mut s)));
                    } else {
                        tokens.push(JsToken::TemplateMiddle(std::mem::take(&mut s)));
                    }
                    // The caller will handle tokenizing the expression until }
                    return Ok(tokens);
                }
                Some('`') => {
                    self.advance();
                    if is_first {
                        // No interpolation at all
                        tokens.push(JsToken::TemplateNoSub(s));
                    } else {
                        tokens.push(JsToken::TemplateTail(s));
                    }
                    return Ok(tokens);
                }
                Some(c) => {
                    self.advance();
                    s.push(c);
                }
            }
        }
    }

    /// Continue reading a template literal after a `}` closes an interpolation.
    fn continue_template(&mut self) -> Result<Vec<JsToken>, String> {
        let start_loc = self.loc();
        let mut tokens = Vec::new();
        let mut s = String::new();

        loop {
            match self.peek() {
                None => {
                    return Err(format!(
                        "{}: unterminated template literal",
                        start_loc.position()
                    ));
                }
                Some('\\') => {
                    self.advance();
                    match self.advance() {
                        Some('n') => s.push('\n'),
                        Some('t') => s.push('\t'),
                        Some('r') => s.push('\r'),
                        Some('\\') => s.push('\\'),
                        Some('`') => s.push('`'),
                        Some('$') => s.push('$'),
                        Some(c) => {
                            s.push('\\');
                            s.push(c);
                        }
                        None => {
                            return Err(format!(
                                "{}: unterminated template escape",
                                start_loc.position()
                            ));
                        }
                    }
                }
                Some('$') if self.peek2() == Some('{') => {
                    self.advance(); // $
                    self.advance(); // {
                    tokens.push(JsToken::TemplateMiddle(std::mem::take(&mut s)));
                    return Ok(tokens);
                }
                Some('`') => {
                    self.advance();
                    tokens.push(JsToken::TemplateTail(s));
                    return Ok(tokens);
                }
                Some(c) => {
                    self.advance();
                    s.push(c);
                }
            }
        }
    }

    fn read_number(&mut self) -> Result<JsToken, String> {
        let start = self.pos;
        let mut is_float = false;

        // Hex literal
        if self.peek() == Some('0') && matches!(self.peek2(), Some('x') | Some('X')) {
            self.advance(); // 0
            self.advance(); // x
            while let Some(c) = self.peek() {
                if c.is_ascii_hexdigit() || c == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
            let s: String = self.input[start..self.pos]
                .iter()
                .filter(|c| **c != '_')
                .collect();
            let val =
                i64::from_str_radix(&s[2..], 16).map_err(|e| format!("bad hex literal: {}", e))?;
            return Ok(JsToken::Int(val));
        }

        // Binary literal
        if self.peek() == Some('0') && matches!(self.peek2(), Some('b') | Some('B')) {
            self.advance(); // 0
            self.advance(); // b
            while let Some(c) = self.peek() {
                if c == '0' || c == '1' || c == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
            let s: String = self.input[start..self.pos]
                .iter()
                .filter(|c| **c != '_')
                .collect();
            let val = i64::from_str_radix(&s[2..], 2)
                .map_err(|e| format!("bad binary literal: {}", e))?;
            return Ok(JsToken::Int(val));
        }

        // Octal literal
        if self.peek() == Some('0') && matches!(self.peek2(), Some('o') | Some('O')) {
            self.advance(); // 0
            self.advance(); // o
            while let Some(c) = self.peek() {
                if ('0'..='7').contains(&c) || c == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
            let s: String = self.input[start..self.pos]
                .iter()
                .filter(|c| **c != '_')
                .collect();
            let val =
                i64::from_str_radix(&s[2..], 8).map_err(|e| format!("bad octal literal: {}", e))?;
            return Ok(JsToken::Int(val));
        }

        // Decimal integer or float
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }

        // Fractional part
        if self.peek() == Some('.') && self.peek2().is_some_and(|c| c.is_ascii_digit()) {
            is_float = true;
            self.advance(); // .
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() || c == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        // Exponent
        if matches!(self.peek(), Some('e') | Some('E')) {
            is_float = true;
            self.advance();
            if matches!(self.peek(), Some('+') | Some('-')) {
                self.advance();
            }
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        let s: String = self.input[start..self.pos]
            .iter()
            .filter(|c| **c != '_')
            .collect();

        if is_float {
            let val: f64 = s.parse().map_err(|e| format!("bad float literal: {}", e))?;
            Ok(JsToken::Float(val))
        } else {
            let val: i64 = s
                .parse()
                .map_err(|e| format!("bad integer literal: {}", e))?;
            Ok(JsToken::Int(val))
        }
    }

    fn read_ident(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' || c == '$' {
                self.advance();
            } else {
                break;
            }
        }
        self.input[start..self.pos].iter().collect()
    }

    fn keyword_or_ident(&self, s: &str) -> JsToken {
        match s {
            "function" => JsToken::Function,
            "return" => JsToken::Return,
            "if" => JsToken::If,
            "else" => JsToken::Else,
            "while" => JsToken::While,
            "for" => JsToken::For,
            "of" => JsToken::Of,
            "in" => JsToken::In,
            "const" => JsToken::Const,
            "let" => JsToken::Let,
            "var" => JsToken::Var,
            "break" => JsToken::Break,
            "continue" => JsToken::Continue,
            "do" => JsToken::Do,
            "switch" => JsToken::Switch,
            "case" => JsToken::Case,
            "default" => JsToken::Default,
            "typeof" => JsToken::Typeof,
            "new" => JsToken::New,
            "throw" => JsToken::Throw,
            "try" => JsToken::Try,
            "catch" => JsToken::Catch,
            "finally" => JsToken::Finally,
            "true" => JsToken::True,
            "false" => JsToken::False,
            "null" => JsToken::Null,
            "undefined" => JsToken::Undefined,
            _ => JsToken::Ident(s.to_string()),
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
            let start_pos = self.pos;

            let c = match self.peek() {
                None => {
                    tokens.push(JsTokenLoc {
                        token: JsToken::Eof,
                        loc,
                        len: 0,
                    });
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
                    let len = self.pos - start_pos;
                    tokens.push(JsTokenLoc {
                        token: tt,
                        loc: loc.clone(),
                        len,
                    });
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
                        let len = self.pos - start_pos;
                        tokens.push(JsTokenLoc {
                            token: tt,
                            loc: loc.clone(),
                            len,
                        });
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
                        self.file, self.line, self.col, c
                    ));
                }
            };

            let len = self.pos - start_pos;
            tokens.push(JsTokenLoc { token, loc, len });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(input: &str) -> Vec<JsToken> {
        let mut lexer = JsLexer::new(input, "<test>");
        lexer
            .tokenize()
            .unwrap()
            .into_iter()
            .map(|t| t.token)
            .collect()
    }

    #[test]
    fn test_basic_tokens() {
        let tokens = lex("const x = 42;");
        assert_eq!(
            tokens,
            vec![
                JsToken::Const,
                JsToken::Ident("x".into()),
                JsToken::Assign,
                JsToken::Int(42),
                JsToken::Semicolon,
                JsToken::Eof
            ]
        );
    }

    #[test]
    fn test_strings() {
        let tokens = lex(r#""hello" 'world'"#);
        assert_eq!(
            tokens,
            vec![
                JsToken::String("hello".into()),
                JsToken::String("world".into()),
                JsToken::Eof
            ]
        );
    }

    #[test]
    fn test_comments() {
        let tokens = lex("x // comment\ny");
        assert_eq!(
            tokens,
            vec![
                JsToken::Ident("x".into()),
                JsToken::Ident("y".into()),
                JsToken::Eof
            ]
        );
    }

    #[test]
    fn test_block_comment() {
        let tokens = lex("x /* block\ncomment */ y");
        assert_eq!(
            tokens,
            vec![
                JsToken::Ident("x".into()),
                JsToken::Ident("y".into()),
                JsToken::Eof
            ]
        );
    }

    #[test]
    fn test_operators() {
        let tokens = lex("=== !== <= >= ==");
        assert_eq!(
            tokens,
            vec![
                JsToken::Eq,
                JsToken::Neq,
                JsToken::Le,
                JsToken::Ge,
                JsToken::EqLoose,
                JsToken::Eof
            ]
        );
    }

    #[test]
    fn test_arrow() {
        let tokens = lex("(x) => x + 1");
        assert_eq!(
            tokens,
            vec![
                JsToken::LParen,
                JsToken::Ident("x".into()),
                JsToken::RParen,
                JsToken::Arrow,
                JsToken::Ident("x".into()),
                JsToken::Plus,
                JsToken::Int(1),
                JsToken::Eof
            ]
        );
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_float() {
        let tokens = lex("3.14 1e10");
        assert_eq!(
            tokens,
            vec![JsToken::Float(3.14), JsToken::Float(1e10), JsToken::Eof]
        );
    }

    #[test]
    fn test_hex() {
        let tokens = lex("0xFF");
        assert_eq!(tokens, vec![JsToken::Int(255), JsToken::Eof]);
    }

    #[test]
    fn test_template_nosub() {
        let tokens = lex("`hello world`");
        assert_eq!(
            tokens,
            vec![JsToken::TemplateNoSub("hello world".into()), JsToken::Eof]
        );
    }

    #[test]
    fn test_template_interpolation() {
        let tokens = lex("`hello ${name}!`");
        assert_eq!(
            tokens,
            vec![
                JsToken::TemplateHead("hello ".into()),
                JsToken::Ident("name".into()),
                JsToken::TemplateTail("!".into()),
                JsToken::Eof
            ]
        );
    }

    #[test]
    fn test_spread() {
        let tokens = lex("...args");
        assert_eq!(
            tokens,
            vec![
                JsToken::DotDotDot,
                JsToken::Ident("args".into()),
                JsToken::Eof
            ]
        );
    }

    #[test]
    fn test_logical_ops() {
        let tokens = lex("a && b || !c");
        assert_eq!(
            tokens,
            vec![
                JsToken::Ident("a".into()),
                JsToken::And,
                JsToken::Ident("b".into()),
                JsToken::Or,
                JsToken::Not,
                JsToken::Ident("c".into()),
                JsToken::Eof
            ]
        );
    }
}
