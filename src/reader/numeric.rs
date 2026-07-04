//! Numeric literal parsing for the Elle lexer.
//!
//! Handles integer, float, and radix (hex/octal/binary) literals with
//! underscore separators and scientific notation.

use super::lexer::Lexer;
use super::token::Token;

/// Validate that a digit-body string does not have leading, trailing, or consecutive underscores.
/// Returns the stripped string (underscores removed) if valid, or an error message.
/// `context` is the full raw literal text used in the error message.
pub(super) fn validate_and_strip_underscores(s: &str, context: &str) -> Result<String, String> {
    if s.starts_with('_') || s.ends_with('_') || s.contains("__") {
        return Err(format!("Invalid underscore in numeric literal: {context}"));
    }
    Ok(s.replace('_', ""))
}

/// The base of an integer literal.
///
/// A distinct type rather than a bare `base: u32` carrying the magic values
/// 2/8/10/16 (freely confusable with any other count). The three facts the
/// lexer needs — the radix for `from_str_radix`, the human name for error
/// messages, and which characters count as digits — live on the type as its
/// single source of truth, so they can't disagree (e.g. a base whose digit set
/// doesn't match its radix).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Radix {
    Binary,
    Octal,
    Decimal,
    Hexadecimal,
}

impl Radix {
    /// Map a base-prefix character (`x`/`o`/`b`, any case) to its radix; `None`
    /// for anything that does not introduce a prefixed literal.
    fn from_prefix_char(c: char) -> Option<Radix> {
        match c.to_ascii_lowercase() {
            'x' => Some(Radix::Hexadecimal),
            'o' => Some(Radix::Octal),
            'b' => Some(Radix::Binary),
            _ => None,
        }
    }

    /// The numeric base passed to `i64::from_str_radix`.
    fn value(self) -> u32 {
        match self {
            Radix::Binary => 2,
            Radix::Octal => 8,
            Radix::Decimal => 10,
            Radix::Hexadecimal => 16,
        }
    }

    /// The name used in `Invalid <name> integer` error messages.
    fn name(self) -> &'static str {
        match self {
            Radix::Binary => "binary",
            Radix::Octal => "octal",
            Radix::Decimal => "decimal",
            Radix::Hexadecimal => "hexadecimal",
        }
    }

    /// Whether `c` is a valid digit in this radix (underscore separators are
    /// handled by the caller, not here).
    fn is_digit(self, c: char) -> bool {
        match self {
            Radix::Binary => matches!(c, '0' | '1'),
            Radix::Octal => matches!(c, '0'..='7'),
            Radix::Decimal => c.is_ascii_digit(),
            Radix::Hexadecimal => c.is_ascii_hexdigit(),
        }
    }

    /// Whether this radix is introduced by a `0x`/`0o`/`0b` prefix, which the
    /// lexer scans and parses differently from plain decimal.
    fn is_prefixed(self) -> bool {
        self != Radix::Decimal
    }
}

impl<'a> Lexer<'a> {
    /// Read a numeric literal (integer, float, or radix-prefixed integer).
    ///
    /// Handles:
    /// - Decimal integers and floats with optional `+`/`-` sign
    /// - Hexadecimal (`0x`/`0X`), octal (`0o`/`0O`), binary (`0b`/`0B`) prefixes
    /// - Underscore separators in digit bodies (validated)
    /// - Scientific notation (`e`/`E` with optional sign)
    pub(super) fn read_number(&mut self) -> Result<Token<'a>, String> {
        let mut raw = String::new();
        let mut sign = String::new();

        // Step 1: consume optional sign
        if let Some(c) = self.current() {
            if c == '+' || c == '-' {
                sign.push(c);
                raw.push(c);
                self.advance();
            }
        }

        // Step 2: detect base prefix (0x, 0o, 0b and uppercase variants)
        let mut radix = Radix::Decimal;
        if self.current() == Some('0') {
            if let Some(prefixed) = self.peek(1).and_then(Radix::from_prefix_char) {
                raw.push('0');
                self.advance(); // consume '0'
                let prefix_char = self.current().unwrap();
                raw.push(prefix_char);
                self.advance(); // consume prefix char
                radix = prefixed;
            }
        }

        // Step 3: collect digit body
        let mut body = String::new();
        let mut has_dot = false;
        let mut has_exp = false;

        if radix.is_prefixed() {
            // Prefixed literal: consume only valid digit chars for the base
            while let Some(c) = self.current() {
                if radix.is_digit(c) || c == '_' {
                    body.push(c);
                    raw.push(c);
                    self.advance();
                } else {
                    break;
                }
            }
            // body must not be empty
            if body.is_empty() {
                return Err(format!("Invalid {} integer: {raw}", radix.name()));
            }
            // The next character must not be an alphanumeric digit — if it is,
            // it's an invalid digit for this base (e.g. '2' after 0b1 is an error).
            if matches!(self.current(), Some(c) if c.is_ascii_alphanumeric()) {
                let bad = self.current().unwrap();
                return Err(format!("Invalid {} integer: {raw}{bad}", radix.name()));
            }
        } else {
            // Decimal: consume leading digits
            while let Some(c) = self.current() {
                if c.is_ascii_digit() || c == '_' {
                    body.push(c);
                    raw.push(c);
                    self.advance();
                } else {
                    break;
                }
            }

            // Optional fractional part
            if self.current() == Some('.') {
                // Check: character immediately before '.' must not be '_'
                if body.ends_with('_') {
                    return Err(format!("Invalid underscore in numeric literal: {raw}."));
                }
                // Peek: character immediately after '.' must not be '_'
                if self.peek(1) == Some('_') {
                    return Err(format!("Invalid underscore in numeric literal: {raw}._"));
                }
                has_dot = true;
                body.push('.');
                raw.push('.');
                self.advance();
                while let Some(c) = self.current() {
                    if c.is_ascii_digit() || c == '_' {
                        body.push(c);
                        raw.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }
            }

            // Optional exponent part (decimal only)
            if matches!(self.current(), Some('e') | Some('E')) {
                // Check: character immediately before 'e'/'E' must not be '_'
                if body.ends_with('_') {
                    return Err(format!("Invalid underscore in numeric literal: {raw}"));
                }
                has_exp = true;
                let e_char = self.current().unwrap();
                body.push(e_char);
                raw.push(e_char);
                self.advance();
                // Optional exponent sign
                if matches!(self.current(), Some('+') | Some('-')) {
                    let sign_char = self.current().unwrap();
                    body.push(sign_char);
                    raw.push(sign_char);
                    self.advance();
                }
                // Check: character immediately after 'e'/'E' (or sign) must not be '_'
                if self.current() == Some('_') {
                    return Err(format!("Invalid underscore in numeric literal: {raw}_"));
                }
                // Must have at least one exponent digit
                if !matches!(self.current(), Some(c) if c.is_ascii_digit()) {
                    return Err(format!("Invalid float: {raw}"));
                }
                while let Some(c) = self.current() {
                    if c.is_ascii_digit() || c == '_' {
                        body.push(c);
                        raw.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
        }

        // Step 4: validate and strip underscores from body
        let stripped_body = validate_and_strip_underscores(&body, &raw)?;

        // Step 5: parse
        if radix.is_prefixed() {
            let n = i64::from_str_radix(&stripped_body, radix.value())
                .map_err(|_| format!("Invalid {} integer: {raw}", radix.name()))?;
            if sign == "-" {
                Ok(Token::Integer(-n))
            } else {
                Ok(Token::Integer(n))
            }
        } else if has_dot || has_exp {
            let full = format!("{sign}{stripped_body}");
            full.parse::<f64>()
                .map(Token::Float)
                .map_err(|_| format!("Invalid float: {raw}"))
        } else {
            let full = format!("{sign}{stripped_body}");
            full.parse::<i64>()
                .map(Token::Integer)
                .map_err(|_| format!("Invalid integer: {raw}"))
        }
    }
}

#[cfg(test)]
mod tests;
