//! Python-surface-syntax tokenizer.
//!
//! Produces `PyToken` values with source locations.  Tracks indentation
//! via an explicit indent stack, emitting synthetic `Indent` and `Dedent`
//! tokens at block boundaries.

use super::token::SourceLoc;

/// Token types for the Python surface syntax.
#[derive(Debug, Clone, PartialEq)]
pub enum PyToken {
    // Literals
    Int(i64),
    Float(f64),
    String(String),
    FString(Vec<FStringPart>),
    True,
    False,
    None,

    // Identifiers
    Ident(String),

    // Keywords
    Def,
    Return,
    If,
    Elif,
    Else,
    While,
    For,
    In,
    And,
    Or,
    Not,
    Break,
    Continue,
    Pass,
    Lambda,
    Class,
    Import,
    From,
    As,
    Try,
    Except,
    Finally,
    Raise,
    With,
    Yield,
    Assert,
    Del,
    Global,
    Nonlocal,
    Is,

    // Operators
    Plus,
    Minus,
    Star,
    StarStar, // **
    Slash,
    SlashSlash, // //
    Percent,
    At,  // @ (decorator / matmul)
    Eq,  // ==
    Neq, // !=
    Lt,
    Gt,
    Le,
    Ge,
    Assign,      // =
    PlusAssign,  // +=
    MinusAssign, // -=
    StarAssign,  // *=
    SlashAssign, // /=
    Dot,
    DotDotDot,   // ... (Ellipsis)
    Arrow,       // ->
    Ampersand,   // &
    Pipe,        // |
    Caret,       // ^
    Tilde,       // ~
    ShiftLeft,   // <<
    ShiftRight,  // >>
    ColonAssign, // :=

    // Delimiters
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Semicolon,

    // Indentation
    Newline,
    Indent,
    Dedent,

    Eof,
}

/// Part of an f-string: either literal text or an interpolated expression.
#[derive(Debug, Clone, PartialEq)]
pub enum FStringPart {
    Lit(String),
    Expr(String),
}

/// A token with its source location and byte length.
#[derive(Debug, Clone)]
pub struct PyTokenLoc {
    pub token: PyToken,
    pub loc: SourceLoc,
    pub len: usize,
}

pub struct PyLexer {
    input: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    file: String,
    /// Stack of indentation levels (in spaces).  Starts with \[0\].
    indent_stack: Vec<usize>,
    /// Nesting depth of brackets/parens/braces (suppresses newlines).
    bracket_depth: u32,
    /// Whether we're at the beginning of a line (need to check indent).
    at_line_start: bool,
}

impl PyLexer {
    pub fn new(input: &str, file: &str) -> Self {
        PyLexer {
            input: input.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            file: file.to_string(),
            indent_stack: vec![0],
            bracket_depth: 0,
            at_line_start: true,
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

    fn read_string(&mut self, quote: char, triple: bool) -> Result<String, String> {
        let start_loc = self.loc();
        if triple {
            // Skip opening triple quote (already consumed first char)
            self.advance();
            self.advance();
        }
        let mut s = String::new();
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
                    Some('a') => s.push('\x07'),
                    Some('b') => s.push('\x08'),
                    Some('f') => s.push('\x0C'),
                    Some('v') => s.push('\x0B'),
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
                    Some('\n') => {} // line continuation
                    Some(c) => {
                        s.push('\\');
                        s.push(c);
                    }
                },
                Some(c) if c == quote => {
                    if triple {
                        if self.peek() == Some(quote) && self.peek2() == Some(quote) {
                            self.advance();
                            self.advance();
                            return Ok(s);
                        }
                        s.push(c);
                    } else {
                        return Ok(s);
                    }
                }
                Some(c) => s.push(c),
            }
        }
    }

    /// Read an f-string, collecting literal segments and `{expr}` interpolations.
    fn read_fstring(&mut self, quote: char, triple: bool) -> Result<Vec<FStringPart>, String> {
        let start_loc = self.loc();
        if triple {
            self.advance();
            self.advance();
        }
        let mut parts = Vec::new();
        let mut lit = String::new();

        loop {
            match self.peek() {
                None => {
                    return Err(format!("{}: unterminated f-string", start_loc.position()));
                }
                Some('\\') => {
                    self.advance();
                    match self.advance() {
                        Some('n') => lit.push('\n'),
                        Some('t') => lit.push('\t'),
                        Some('r') => lit.push('\r'),
                        Some('\\') => lit.push('\\'),
                        Some('\'') => lit.push('\''),
                        Some('"') => lit.push('"'),
                        Some('{') => lit.push('{'),
                        Some('}') => lit.push('}'),
                        Some(c) => {
                            lit.push('\\');
                            lit.push(c);
                        }
                        None => {
                            return Err(format!(
                                "{}: unterminated f-string escape",
                                start_loc.position()
                            ));
                        }
                    }
                }
                Some('{') if self.peek2() == Some('{') => {
                    // Escaped brace: {{ → {
                    self.advance();
                    self.advance();
                    lit.push('{');
                }
                Some('{') => {
                    self.advance();
                    if !lit.is_empty() {
                        parts.push(FStringPart::Lit(std::mem::take(&mut lit)));
                    }
                    // Read expression until matching }
                    let mut depth = 1u32;
                    let mut expr = String::new();
                    while depth > 0 {
                        match self.advance() {
                            None => {
                                return Err(format!(
                                    "{}: unterminated f-string expression",
                                    start_loc.position()
                                ));
                            }
                            Some('{') => {
                                depth += 1;
                                expr.push('{');
                            }
                            Some('}') => {
                                depth -= 1;
                                if depth > 0 {
                                    expr.push('}');
                                }
                            }
                            Some(c) => expr.push(c),
                        }
                    }
                    parts.push(FStringPart::Expr(expr));
                }
                Some('}') if self.peek2() == Some('}') => {
                    self.advance();
                    self.advance();
                    lit.push('}');
                }
                Some(c) if c == quote => {
                    self.advance();
                    if triple {
                        if self.peek() == Some(quote) && self.peek2() == Some(quote) {
                            self.advance();
                            self.advance();
                            if !lit.is_empty() {
                                parts.push(FStringPart::Lit(lit));
                            }
                            return Ok(parts);
                        }
                        lit.push(c);
                    } else {
                        if !lit.is_empty() {
                            parts.push(FStringPart::Lit(lit));
                        }
                        return Ok(parts);
                    }
                }
                Some(c) => {
                    self.advance();
                    lit.push(c);
                }
            }
        }
    }

    fn read_number(&mut self) -> Result<PyToken, String> {
        let start = self.pos;
        let mut is_float = false;

        // Hex literal
        if self.peek() == Some('0') && matches!(self.peek2(), Some('x') | Some('X')) {
            self.advance();
            self.advance();
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
            return Ok(PyToken::Int(val));
        }

        // Binary literal
        if self.peek() == Some('0') && matches!(self.peek2(), Some('b') | Some('B')) {
            self.advance();
            self.advance();
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
            return Ok(PyToken::Int(val));
        }

        // Octal literal
        if self.peek() == Some('0') && matches!(self.peek2(), Some('o') | Some('O')) {
            self.advance();
            self.advance();
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
            return Ok(PyToken::Int(val));
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
            self.advance();
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
            Ok(PyToken::Float(val))
        } else {
            let val: i64 = s
                .parse()
                .map_err(|e| format!("bad integer literal: {}", e))?;
            Ok(PyToken::Int(val))
        }
    }

    fn read_ident(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }
        self.input[start..self.pos].iter().collect()
    }

    fn keyword_or_ident(&self, s: &str) -> PyToken {
        match s {
            "def" => PyToken::Def,
            "return" => PyToken::Return,
            "if" => PyToken::If,
            "elif" => PyToken::Elif,
            "else" => PyToken::Else,
            "while" => PyToken::While,
            "for" => PyToken::For,
            "in" => PyToken::In,
            "and" => PyToken::And,
            "or" => PyToken::Or,
            "not" => PyToken::Not,
            "break" => PyToken::Break,
            "continue" => PyToken::Continue,
            "pass" => PyToken::Pass,
            "lambda" => PyToken::Lambda,
            "class" => PyToken::Class,
            "import" => PyToken::Import,
            "from" => PyToken::From,
            "as" => PyToken::As,
            "try" => PyToken::Try,
            "except" => PyToken::Except,
            "finally" => PyToken::Finally,
            "raise" => PyToken::Raise,
            "with" => PyToken::With,
            "yield" => PyToken::Yield,
            "assert" => PyToken::Assert,
            "del" => PyToken::Del,
            "global" => PyToken::Global,
            "nonlocal" => PyToken::Nonlocal,
            "is" => PyToken::Is,
            "True" => PyToken::True,
            "False" => PyToken::False,
            "None" => PyToken::None,
            _ => PyToken::Ident(s.to_string()),
        }
    }

    /// Measure indentation at the current position (start of line).
    /// Returns the number of spaces (tabs count as 4 spaces).
    fn measure_indent(&self) -> usize {
        let mut spaces = 0;
        let mut i = self.pos;
        while i < self.input.len() {
            match self.input[i] {
                ' ' => {
                    spaces += 1;
                    i += 1;
                }
                '\t' => {
                    spaces += 4;
                    i += 1;
                }
                _ => break,
            }
        }
        spaces
    }

    /// Tokenize the entire input, returning all tokens with locations.
    pub fn tokenize(&mut self) -> Result<Vec<PyTokenLoc>, String> {
        let mut tokens = Vec::new();

        loop {
            // Handle indentation at the start of a line
            if self.at_line_start && self.bracket_depth == 0 {
                self.at_line_start = false;

                // Skip blank lines and comment-only lines
                loop {
                    let mut i = self.pos;
                    // Skip spaces/tabs
                    while i < self.input.len() && (self.input[i] == ' ' || self.input[i] == '\t') {
                        i += 1;
                    }
                    if i < self.input.len() && self.input[i] == '#' {
                        // Comment-only line: skip to next line
                        while i < self.input.len() && self.input[i] != '\n' {
                            i += 1;
                        }
                        if i < self.input.len() {
                            i += 1; // skip \n
                        }
                        // Update position, line, col
                        while self.pos < i {
                            self.advance();
                        }
                        continue;
                    }
                    if i < self.input.len() && self.input[i] == '\n' {
                        // Blank line: skip
                        while self.pos <= i {
                            self.advance();
                        }
                        continue;
                    }
                    break;
                }

                if self.peek().is_none() {
                    // EOF — emit dedents for all remaining indent levels
                    let loc = self.loc();
                    while self.indent_stack.len() > 1 {
                        self.indent_stack.pop();
                        tokens.push(PyTokenLoc {
                            token: PyToken::Dedent,
                            loc: loc.clone(),
                            len: 0,
                        });
                    }
                    tokens.push(PyTokenLoc {
                        token: PyToken::Eof,
                        loc,
                        len: 0,
                    });
                    return Ok(tokens);
                }

                let indent = self.measure_indent();
                let current = *self.indent_stack.last().unwrap();

                if indent > current {
                    self.indent_stack.push(indent);
                    tokens.push(PyTokenLoc {
                        token: PyToken::Indent,
                        loc: self.loc(),
                        len: 0,
                    });
                } else if indent < current {
                    while *self.indent_stack.last().unwrap() > indent {
                        self.indent_stack.pop();
                        tokens.push(PyTokenLoc {
                            token: PyToken::Dedent,
                            loc: self.loc(),
                            len: 0,
                        });
                    }
                    if *self.indent_stack.last().unwrap() != indent {
                        return Err(format!(
                            "{}:{}:{}: inconsistent dedent",
                            self.file, self.line, self.col
                        ));
                    }
                }

                // Skip the whitespace we just measured
                while self.peek().is_some_and(|c| c == ' ' || c == '\t') {
                    self.advance();
                }
                continue;
            }

            // Skip spaces (not newlines, not tabs at line start)
            while self.peek().is_some_and(|c| c == ' ' || c == '\t') {
                self.advance();
            }

            let loc = self.loc();
            let start_pos = self.pos;

            let c = match self.peek() {
                None => {
                    // EOF — emit remaining dedents
                    while self.indent_stack.len() > 1 {
                        self.indent_stack.pop();
                        tokens.push(PyTokenLoc {
                            token: PyToken::Dedent,
                            loc: loc.clone(),
                            len: 0,
                        });
                    }
                    tokens.push(PyTokenLoc {
                        token: PyToken::Eof,
                        loc,
                        len: 0,
                    });
                    return Ok(tokens);
                }
                Some(c) => c,
            };

            // Newline
            if c == '\n' {
                self.advance();
                if self.bracket_depth == 0 {
                    // Only emit Newline if previous token wasn't already Newline
                    let should_emit = tokens
                        .last()
                        .map(|t| !matches!(t.token, PyToken::Newline | PyToken::Indent))
                        .unwrap_or(false);
                    if should_emit {
                        tokens.push(PyTokenLoc {
                            token: PyToken::Newline,
                            loc: loc.clone(),
                            len: 1,
                        });
                    }
                    self.at_line_start = true;
                }
                continue;
            }

            // Line continuation
            if c == '\\' && self.peek2() == Some('\n') {
                self.advance(); // backslash
                self.advance(); // newline
                continue;
            }

            let token = match c {
                // Comments
                '#' => {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.advance();
                    }
                    continue;
                }

                // String literals (including f-strings)
                'f' | 'F' if matches!(self.peek2(), Some('"') | Some('\'')) => {
                    self.advance(); // skip 'f'
                    let quote = self.peek().unwrap();
                    self.advance(); // skip opening quote
                    let triple = self.peek() == Some(quote) && self.peek2() == Some(quote);
                    let parts = self.read_fstring(quote, triple)?;
                    PyToken::FString(parts)
                }
                'r' | 'R' if matches!(self.peek2(), Some('"') | Some('\'')) => {
                    self.advance(); // skip 'r'
                    let quote = self.peek().unwrap();
                    self.advance(); // skip opening quote
                                    // Raw string: no escape processing
                    let triple = self.peek() == Some(quote) && self.peek2() == Some(quote);
                    if triple {
                        self.advance();
                        self.advance();
                    }
                    let mut s = String::new();
                    loop {
                        match self.advance() {
                            None => {
                                return Err(format!("{}: unterminated raw string", loc.position()));
                            }
                            Some(c) if c == quote => {
                                if triple {
                                    if self.peek() == Some(quote) && self.peek2() == Some(quote) {
                                        self.advance();
                                        self.advance();
                                        break;
                                    }
                                    s.push(c);
                                } else {
                                    break;
                                }
                            }
                            Some(c) => s.push(c),
                        }
                    }
                    PyToken::String(s)
                }
                'b' | 'B' if matches!(self.peek2(), Some('"') | Some('\'')) => {
                    // Byte strings — treat as regular strings for now
                    self.advance();
                    let quote = self.peek().unwrap();
                    self.advance();
                    let triple = self.peek() == Some(quote) && self.peek2() == Some(quote);
                    let s = self.read_string(quote, triple)?;
                    PyToken::String(s)
                }
                '"' | '\'' => {
                    self.advance(); // skip opening quote
                    let triple = self.peek() == Some(c) && self.peek2() == Some(c);
                    let s = self.read_string(c, triple)?;
                    PyToken::String(s)
                }

                // Numbers
                '0'..='9' => self.read_number()?,

                // Identifiers and keywords
                c if c.is_alphabetic() || c == '_' => {
                    let name = self.read_ident();
                    self.keyword_or_ident(&name)
                }

                // Three-char operators
                '.' if self.peek2() == Some('.') && self.peek3() == Some('.') => {
                    self.advance();
                    self.advance();
                    self.advance();
                    PyToken::DotDotDot
                }
                '*' if self.peek2() == Some('*') => {
                    self.advance();
                    self.advance();
                    PyToken::StarStar
                }
                '/' if self.peek2() == Some('/') => {
                    self.advance();
                    self.advance();
                    PyToken::SlashSlash
                }

                // Two-char operators
                '=' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::Eq
                }
                '!' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::Neq
                }
                '<' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::Le
                }
                '>' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::Ge
                }
                '<' if self.peek2() == Some('<') => {
                    self.advance();
                    self.advance();
                    PyToken::ShiftLeft
                }
                '>' if self.peek2() == Some('>') => {
                    self.advance();
                    self.advance();
                    PyToken::ShiftRight
                }
                '+' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::PlusAssign
                }
                '-' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::MinusAssign
                }
                '*' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::StarAssign
                }
                '/' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::SlashAssign
                }
                '-' if self.peek2() == Some('>') => {
                    self.advance();
                    self.advance();
                    PyToken::Arrow
                }
                ':' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::ColonAssign
                }

                // Single-char operators and delimiters
                '+' => {
                    self.advance();
                    PyToken::Plus
                }
                '-' => {
                    self.advance();
                    PyToken::Minus
                }
                '*' => {
                    self.advance();
                    PyToken::Star
                }
                '/' => {
                    self.advance();
                    PyToken::Slash
                }
                '%' => {
                    self.advance();
                    PyToken::Percent
                }
                '@' => {
                    self.advance();
                    PyToken::At
                }
                '<' => {
                    self.advance();
                    PyToken::Lt
                }
                '>' => {
                    self.advance();
                    PyToken::Gt
                }
                '=' => {
                    self.advance();
                    PyToken::Assign
                }
                '.' => {
                    self.advance();
                    PyToken::Dot
                }
                '&' => {
                    self.advance();
                    PyToken::Ampersand
                }
                '|' => {
                    self.advance();
                    PyToken::Pipe
                }
                '^' => {
                    self.advance();
                    PyToken::Caret
                }
                '~' => {
                    self.advance();
                    PyToken::Tilde
                }
                ':' => {
                    self.advance();
                    PyToken::Colon
                }
                ';' => {
                    self.advance();
                    PyToken::Semicolon
                }
                ',' => {
                    self.advance();
                    PyToken::Comma
                }
                '(' => {
                    self.advance();
                    self.bracket_depth += 1;
                    PyToken::LParen
                }
                ')' => {
                    self.advance();
                    self.bracket_depth = self.bracket_depth.saturating_sub(1);
                    PyToken::RParen
                }
                '[' => {
                    self.advance();
                    self.bracket_depth += 1;
                    PyToken::LBracket
                }
                ']' => {
                    self.advance();
                    self.bracket_depth = self.bracket_depth.saturating_sub(1);
                    PyToken::RBracket
                }
                '{' => {
                    self.advance();
                    self.bracket_depth += 1;
                    PyToken::LBrace
                }
                '}' => {
                    self.advance();
                    self.bracket_depth = self.bracket_depth.saturating_sub(1);
                    PyToken::RBrace
                }

                _ => {
                    return Err(format!(
                        "{}:{}:{}: unexpected character '{}'",
                        self.file, self.line, self.col, c
                    ));
                }
            };

            let len = self.pos - start_pos;
            tokens.push(PyTokenLoc { token, loc, len });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(input: &str) -> Vec<PyToken> {
        let mut lexer = PyLexer::new(input, "<test>");
        lexer
            .tokenize()
            .unwrap()
            .into_iter()
            .map(|t| t.token)
            .collect()
    }

    #[test]
    fn test_basic_tokens() {
        let tokens = lex("x = 42\n");
        assert_eq!(
            tokens,
            vec![
                PyToken::Ident("x".into()),
                PyToken::Assign,
                PyToken::Int(42),
                PyToken::Newline,
                PyToken::Eof
            ]
        );
    }

    #[test]
    fn test_indent_dedent() {
        let tokens = lex("if True:\n  x = 1\ny = 2\n");
        assert_eq!(
            tokens,
            vec![
                PyToken::If,
                PyToken::True,
                PyToken::Colon,
                PyToken::Newline,
                PyToken::Indent,
                PyToken::Ident("x".into()),
                PyToken::Assign,
                PyToken::Int(1),
                PyToken::Newline,
                PyToken::Dedent,
                PyToken::Ident("y".into()),
                PyToken::Assign,
                PyToken::Int(2),
                PyToken::Newline,
                PyToken::Eof
            ]
        );
    }

    #[test]
    fn test_strings() {
        let tokens = lex("\"hello\" 'world'\n");
        assert_eq!(
            tokens,
            vec![
                PyToken::String("hello".into()),
                PyToken::String("world".into()),
                PyToken::Newline,
                PyToken::Eof
            ]
        );
    }

    #[test]
    fn test_comments() {
        let tokens = lex("x # comment\ny\n");
        assert_eq!(
            tokens,
            vec![
                PyToken::Ident("x".into()),
                PyToken::Newline,
                PyToken::Ident("y".into()),
                PyToken::Newline,
                PyToken::Eof
            ]
        );
    }

    #[test]
    fn test_operators() {
        let tokens = lex("== != <= >= **\n");
        assert_eq!(
            tokens,
            vec![
                PyToken::Eq,
                PyToken::Neq,
                PyToken::Le,
                PyToken::Ge,
                PyToken::StarStar,
                PyToken::Newline,
                PyToken::Eof
            ]
        );
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_float() {
        let tokens = lex("3.14 1e10\n");
        assert_eq!(
            tokens,
            vec![
                PyToken::Float(3.14),
                PyToken::Float(1e10),
                PyToken::Newline,
                PyToken::Eof
            ]
        );
    }

    #[test]
    fn test_hex() {
        let tokens = lex("0xFF\n");
        assert_eq!(
            tokens,
            vec![PyToken::Int(255), PyToken::Newline, PyToken::Eof]
        );
    }

    #[test]
    fn test_bracket_suppresses_newline() {
        let tokens = lex("[1,\n2]\n");
        assert_eq!(
            tokens,
            vec![
                PyToken::LBracket,
                PyToken::Int(1),
                PyToken::Comma,
                PyToken::Int(2),
                PyToken::RBracket,
                PyToken::Newline,
                PyToken::Eof
            ]
        );
    }

    #[test]
    fn test_fstring() {
        let tokens = lex("f\"hello {name}\"\n");
        assert_eq!(
            tokens,
            vec![
                PyToken::FString(vec![
                    FStringPart::Lit("hello ".into()),
                    FStringPart::Expr("name".into()),
                ]),
                PyToken::Newline,
                PyToken::Eof
            ]
        );
    }

    #[test]
    fn test_logical_ops() {
        let tokens = lex("a and b or not c\n");
        assert_eq!(
            tokens,
            vec![
                PyToken::Ident("a".into()),
                PyToken::And,
                PyToken::Ident("b".into()),
                PyToken::Or,
                PyToken::Not,
                PyToken::Ident("c".into()),
                PyToken::Newline,
                PyToken::Eof
            ]
        );
    }

    #[test]
    fn test_nested_indent() {
        let tokens = lex("if True:\n  if True:\n    x = 1\n");
        assert_eq!(
            tokens,
            vec![
                PyToken::If,
                PyToken::True,
                PyToken::Colon,
                PyToken::Newline,
                PyToken::Indent,
                PyToken::If,
                PyToken::True,
                PyToken::Colon,
                PyToken::Newline,
                PyToken::Indent,
                PyToken::Ident("x".into()),
                PyToken::Assign,
                PyToken::Int(1),
                PyToken::Newline,
                PyToken::Dedent,
                PyToken::Dedent,
                PyToken::Eof
            ]
        );
    }

    #[test]
    fn test_triple_quoted_string() {
        let tokens = lex("\"\"\"hello\nworld\"\"\"\n");
        assert_eq!(
            tokens,
            vec![
                PyToken::String("hello\nworld".into()),
                PyToken::Newline,
                PyToken::Eof
            ]
        );
    }
}
