// audited: 2026-09-23
//! The escapes a string literal may carry, and the spelling of a string that
//! every lexicon reads back.
//!
//! docs/syntax.md
//! docs/impl/lexicon.md

use std::fmt::{self, Write};

/// The escapes every lexicon reads, as (the character after the backslash,
/// the character the escape stands for). The decoder and the printer both
/// read this one table, so what one writes the other reads.
const SHARED: [(char, char); 5] = [
    ('n', '\n'),
    ('t', '\t'),
    ('r', '\r'),
    ('\\', '\\'),
    ('"', '"'),
];

/// Which escapes a string literal accepts, and what the reader does with an
/// escape it does not know. A `Lexicon` field (docs/impl/lexicon.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StringEscapes {
    /// The five shared escapes. Any other escape drops its backslash and
    /// keeps the character after it. Epochs 0 to 12.
    Lenient,
    /// The five shared escapes, `\0`, `\xHH` from `00` to `7f`, and `\u{H…}`
    /// for a Unicode scalar value. Any other escape is an error. Epoch 13.
    Strict,
}

/// One decoded escape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Decoded {
    /// The character the escape stands for.
    pub ch: char,
    /// The bytes of source the escape spans after its backslash.
    pub len: usize,
}

impl StringEscapes {
    /// Decode the escape at the start of `after`, the source text that
    /// follows a backslash. The error names the escape, and the caller adds
    /// where it is.
    pub(crate) fn decode(self, after: &str) -> Result<Decoded, String> {
        let c = after
            .chars()
            .next()
            .ok_or_else(|| "Unterminated string escape".to_string())?;
        if let Some(&(_, ch)) = SHARED.iter().find(|(escape, _)| *escape == c) {
            return Ok(Decoded { ch, len: 1 });
        }
        match self {
            StringEscapes::Lenient => Ok(Decoded {
                ch: c,
                len: c.len_utf8(),
            }),
            StringEscapes::Strict => match c {
                '0' => Ok(Decoded { ch: '\0', len: 1 }),
                'x' => decode_hex(after),
                'u' => decode_unicode(after),
                _ => Err(unknown(c)),
            },
        }
    }

    /// The spelling under `target` of the string literal `lexeme`, which
    /// `self` read. `None` when every escape in it decodes alike under both.
    ///
    /// An escape both decode alike keeps its text. Any other escape becomes
    /// the character `self` read from it, spelled as [`StringLiteral`]
    /// spells it, so the new text reads under `target` as the old text read
    /// under `self`.
    pub(crate) fn respell(
        self,
        lexeme: &str,
        target: StringEscapes,
    ) -> Result<Option<String>, String> {
        let body = lexeme
            .strip_prefix('"')
            .and_then(|l| l.strip_suffix('"'))
            .ok_or_else(|| format!("{lexeme:?} is not a string literal"))?;
        let mut out = String::with_capacity(lexeme.len());
        out.push('"');
        let mut changed = false;
        let mut rest = body;
        while let Some(at) = rest.find('\\') {
            out.push_str(&rest[..at]);
            let after = &rest[at + 1..];
            let escape = self.decode(after)?;
            if target.decode(after) == Ok(escape) {
                out.push_str(&rest[at..at + 1 + escape.len]);
            } else {
                write_char(&mut out, escape.ch).map_err(|e| e.to_string())?;
                changed = true;
            }
            rest = &after[escape.len..];
        }
        out.push_str(rest);
        out.push('"');
        Ok(changed.then_some(out))
    }
}

/// `\xHH`, where `after` starts at the `x`.
fn decode_hex(after: &str) -> Result<Decoded, String> {
    let Some(digits) = after
        .get(1..3)
        .filter(|d| d.bytes().all(|b| b.is_ascii_hexdigit()))
    else {
        return Err("string escape `\\x` takes exactly two hex digits".to_string());
    };
    match u8::from_str_radix(digits, 16) {
        Ok(byte) if byte <= 0x7f => Ok(Decoded {
            ch: char::from(byte),
            len: 3,
        }),
        _ => Err(format!(
            "string escape `\\x{digits}` is above `\\x7f`: a string holds characters, \
             so write `\\u{{{digits}}}` for U+00{}, or use bytes for a raw byte",
            digits.to_ascii_uppercase()
        )),
    }
}

/// `\u{H…}`, where `after` starts at the `u`.
fn decode_unicode(after: &str) -> Result<Decoded, String> {
    let shape = || "string escape `\\u` takes one to six hex digits in braces, as in `\\u{e9}`";
    let inner = after[1..].strip_prefix('{').ok_or_else(shape)?;
    let n = inner.bytes().take_while(u8::is_ascii_hexdigit).count();
    if n == 0 || n > 6 || !inner[n..].starts_with('}') {
        return Err(shape().to_string());
    }
    let digits = &inner[..n];
    u32::from_str_radix(digits, 16)
        .ok()
        .and_then(char::from_u32)
        .map(|ch| Decoded { ch, len: n + 3 })
        .ok_or_else(|| format!("string escape `\\u{{{digits}}}` is not a Unicode scalar value"))
}

/// The error for an escape the strict rules do not know.
fn unknown(c: char) -> String {
    if c.is_control() || c.is_whitespace() {
        format!(
            "unknown string escape: a backslash before U+{:04X}",
            u32::from(c)
        )
    } else {
        format!("unknown string escape `\\{c}`")
    }
}

/// Write `ch` as it appears inside a string literal: as its shared escape
/// when it has one, and as itself otherwise.
fn write_char(out: &mut impl Write, ch: char) -> fmt::Result {
    match SHARED.iter().find(|(_, c)| *c == ch) {
        Some(&(escape, _)) => {
            out.write_char('\\')?;
            out.write_char(escape)
        }
        None => out.write_char(ch),
    }
}

/// A string printed as a literal, quotes included, that every lexicon reads
/// back to the same string (docs/impl/lexicon.md).
///
/// Every lexicon reads the five shared escapes alike and takes any other
/// character inside the quotes as itself, so this writes those five escapes
/// and nothing else.
pub struct StringLiteral<'a>(pub &'a str);

impl fmt::Display for StringLiteral<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_char('"')?;
        for ch in self.0.chars() {
            write_char(f, ch)?;
        }
        f.write_char('"')
    }
}
