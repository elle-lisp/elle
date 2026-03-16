//! Format spec parser for `string/format`.
//!
//! Parses format specification strings like `>10.2f`, `05d`, `*^20`.
//! Used by `format.rs` to determine alignment, padding, precision, and type.
use crate::value::fiber::{SignalBits, SIG_ERROR};
use crate::value::{error_val, Value};

// ============================================================================
// Types
// ============================================================================

/// Parsed format specification.
pub(super) struct FormatSpec {
    pub(super) fill: char,
    pub(super) align: Align,
    pub(super) width: Option<usize>,
    pub(super) precision: Option<usize>,
    pub(super) ty: FormatType,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Align {
    Left,
    Right,
    Center,
    Default,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum FormatType {
    None,
    Decimal,
    Hex,
    HexUpper,
    Octal,
    Binary,
    Float,
    Scientific,
    StringType,
}

// ============================================================================
// Format spec parsing
// ============================================================================

/// Parse a format spec string like `>10.2f` or `05d` or `*^20`.
pub(super) fn parse_format_spec(spec: &str) -> Result<FormatSpec, (SignalBits, Value)> {
    if spec.is_empty() {
        return Ok(FormatSpec {
            fill: ' ',
            align: Align::Default,
            width: None,
            precision: None,
            ty: FormatType::None,
        });
    }

    let chars: Vec<char> = spec.chars().collect();
    let mut pos = 0;

    // --- Fill and align ---
    let mut fill = ' ';
    let mut align = Align::Default;

    if chars.len() >= 2 && is_align_char(chars[1]) {
        // Two-char fill+align: e.g. `*^`, `0>`
        fill = chars[0];
        align = parse_align_char(chars[1]);
        pos = 2;
    } else if !chars.is_empty() && is_align_char(chars[0]) {
        // Single-char align: e.g. `>`
        align = parse_align_char(chars[0]);
        pos = 1;
    } else if chars.len() >= 2 && chars[0] == '0' && chars[1].is_ascii_digit() {
        // Zero-padding shorthand: `05d` means fill='0', align=right, width=5
        fill = '0';
        align = Align::Right;
        // Don't advance pos — the `0` is consumed as fill, digits parsed as width below
        pos = 1;
    }

    // --- Width ---
    let mut width = None;
    let width_start = pos;
    while pos < chars.len() && chars[pos].is_ascii_digit() {
        pos += 1;
    }
    if pos > width_start {
        let width_str: String = chars[width_start..pos].iter().collect();
        width = Some(width_str.parse::<usize>().map_err(|_| {
            (
                SIG_ERROR,
                error_val(
                    "format-error",
                    format!("string/format: invalid format spec '{}'", spec),
                ),
            )
        })?);
    }

    // --- Precision ---
    let mut precision = None;
    if pos < chars.len() && chars[pos] == '.' {
        pos += 1; // skip '.'
        let prec_start = pos;
        while pos < chars.len() && chars[pos].is_ascii_digit() {
            pos += 1;
        }
        if pos > prec_start {
            let prec_str: String = chars[prec_start..pos].iter().collect();
            precision = Some(prec_str.parse::<usize>().map_err(|_| {
                (
                    SIG_ERROR,
                    error_val(
                        "format-error",
                        format!("string/format: invalid format spec '{}'", spec),
                    ),
                )
            })?);
        } else {
            return Err((
                SIG_ERROR,
                error_val(
                    "format-error",
                    format!("string/format: invalid format spec '{}'", spec),
                ),
            ));
        }
    }

    // --- Type ---
    let mut ty = FormatType::None;
    if pos < chars.len() {
        ty = match chars[pos] {
            'd' => FormatType::Decimal,
            'x' => FormatType::Hex,
            'X' => FormatType::HexUpper,
            'o' => FormatType::Octal,
            'b' => FormatType::Binary,
            'f' => FormatType::Float,
            'e' => FormatType::Scientific,
            's' => FormatType::StringType,
            _ => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "format-error",
                        format!("string/format: invalid format spec '{}'", spec),
                    ),
                ))
            }
        };
        pos += 1;
    }

    // Anything remaining is invalid
    if pos < chars.len() {
        return Err((
            SIG_ERROR,
            error_val(
                "format-error",
                format!("string/format: invalid format spec '{}'", spec),
            ),
        ));
    }

    Ok(FormatSpec {
        fill,
        align,
        width,
        precision,
        ty,
    })
}

fn is_align_char(c: char) -> bool {
    matches!(c, '<' | '>' | '^')
}

fn parse_align_char(c: char) -> Align {
    match c {
        '<' => Align::Left,
        '>' => Align::Right,
        '^' => Align::Center,
        _ => Align::Default,
    }
}

/// Return the type character for error messages.
pub(super) fn spec_type_char(ty: FormatType) -> &'static str {
    match ty {
        FormatType::None => "",
        FormatType::Decimal => "d",
        FormatType::Hex => "x",
        FormatType::HexUpper => "X",
        FormatType::Octal => "o",
        FormatType::Binary => "b",
        FormatType::Float => "f",
        FormatType::Scientific => "e",
        FormatType::StringType => "s",
    }
}
