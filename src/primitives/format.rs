//! String formatting primitive
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

// ============================================================================
// Types
// ============================================================================

/// A parsed placeholder from the template string.
struct Placeholder<'a> {
    /// Name of the placeholder (empty string for positional `{}`).
    name: &'a str,
    /// Raw format spec string (everything after `:`, empty if no spec).
    spec: &'a str,
    /// Byte offset of the opening `{` in the template.
    start: usize,
    /// Byte offset one past the closing `}` in the template.
    end: usize,
}

/// Parsed format specification.
struct FormatSpec {
    fill: char,
    align: Align,
    width: Option<usize>,
    precision: Option<usize>,
    ty: FormatType,
}

#[derive(Clone, Copy, PartialEq)]
enum Align {
    Left,
    Right,
    Center,
    Default,
}

#[derive(Clone, Copy, PartialEq)]
enum FormatType {
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
// Template parsing
// ============================================================================

/// Parse template string into literal segments and placeholders.
///
/// Handles `{{` as escaped `{` and `}}` as escaped `}`.
/// Returns a list of placeholders with their byte positions.
fn parse_placeholders(template: &str) -> Result<Vec<Placeholder<'_>>, (SignalBits, Value)> {
    let mut placeholders = Vec::new();
    let bytes = template.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'{' {
            // Escaped brace: `{{`
            if i + 1 < len && bytes[i + 1] == b'{' {
                i += 2;
                continue;
            }
            // Find matching `}`
            let start = i;
            i += 1; // skip `{`
            let content_start = i;
            while i < len && bytes[i] != b'}' {
                i += 1;
            }
            if i >= len {
                return Err((
                    SIG_ERROR,
                    error_val("format-error", "string/format: unmatched '{' in template"),
                ));
            }
            let content = &template[content_start..i];
            i += 1; // skip `}`
            let end = i;

            // Split content on `:` to get name and spec
            let (name, spec) = match content.find(':') {
                Some(colon_pos) => (&content[..colon_pos], &content[colon_pos + 1..]),
                None => (content, ""),
            };

            placeholders.push(Placeholder {
                name,
                spec,
                start,
                end,
            });
        } else if bytes[i] == b'}' {
            // Escaped brace: `}}`
            if i + 1 < len && bytes[i + 1] == b'}' {
                i += 2;
                continue;
            }
            return Err((
                SIG_ERROR,
                error_val("format-error", "string/format: unmatched '}' in template"),
            ));
        } else {
            i += 1;
        }
    }

    Ok(placeholders)
}

// ============================================================================
// Format spec parsing
// ============================================================================

/// Parse a format spec string like `>10.2f` or `05d` or `*^20`.
fn parse_format_spec(spec: &str) -> Result<FormatSpec, (SignalBits, Value)> {
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

// ============================================================================
// Value formatting
// ============================================================================

/// Format a single value according to a parsed format spec.
fn format_value(value: &Value, spec_str: &str) -> Result<String, (SignalBits, Value)> {
    let mut spec = parse_format_spec(spec_str)?;

    // Resolve default alignment based on value type:
    // numbers default to right-align, everything else to left-align.
    if spec.align == Align::Default {
        let is_numeric = value.as_int().is_some() || value.as_float().is_some();
        spec.align = if is_numeric {
            Align::Right
        } else {
            Align::Left
        };
    }

    // Get the raw formatted string (before width/align)
    let raw = format_raw(value, &spec)?;

    // Apply width and alignment
    apply_width_align(&raw, &spec)
}

/// Format the value's content without width/alignment padding.
fn format_raw(value: &Value, spec: &FormatSpec) -> Result<String, (SignalBits, Value)> {
    // Integer formatting
    if let Some(n) = value.as_int() {
        return format_int(n, spec);
    }

    // Float formatting
    if let Some(f) = value.as_float() {
        return format_float(f, spec);
    }

    // String formatting
    if value.is_string() {
        return value
            .with_string(|s| format_string(s, spec))
            .unwrap_or_else(|| Ok(String::new()));
    }

    // For all other types: only None or StringType specs are valid
    match spec.ty {
        FormatType::None | FormatType::StringType => {
            let mut s = String::new();
            use std::fmt::Write;
            let _ = write!(s, "{}", value);
            if let Some(prec) = spec.precision {
                let truncated: String = s.chars().take(prec).collect();
                return Ok(truncated);
            }
            Ok(s)
        }
        _ => Err((
            SIG_ERROR,
            error_val(
                "format-error",
                format!(
                    "string/format: cannot format {} with spec '{}'",
                    value.type_name(),
                    spec_type_char(spec.ty)
                ),
            ),
        )),
    }
}

fn format_int(n: i64, spec: &FormatSpec) -> Result<String, (SignalBits, Value)> {
    match spec.ty {
        FormatType::None | FormatType::Decimal => Ok(format!("{}", n)),
        FormatType::Hex => Ok(format!("{:x}", n)),
        FormatType::HexUpper => Ok(format!("{:X}", n)),
        FormatType::Octal => Ok(format!("{:o}", n)),
        FormatType::Binary => Ok(format!("{:b}", n)),
        FormatType::Float => {
            let f = n as f64;
            match spec.precision {
                Some(prec) => Ok(format!("{:.prec$}", f, prec = prec)),
                None => Ok(format!("{:.1}", f)),
            }
        }
        FormatType::Scientific => {
            let f = n as f64;
            match spec.precision {
                Some(prec) => Ok(format!("{:.prec$e}", f, prec = prec)),
                None => Ok(format!("{:e}", f)),
            }
        }
        _ => Err((
            SIG_ERROR,
            error_val(
                "format-error",
                format!(
                    "string/format: cannot format integer with spec '{}'",
                    spec_type_char(spec.ty)
                ),
            ),
        )),
    }
}

fn format_float(f: f64, spec: &FormatSpec) -> Result<String, (SignalBits, Value)> {
    match spec.ty {
        FormatType::None | FormatType::Float => match spec.precision {
            Some(prec) => Ok(format!("{:.prec$}", f, prec = prec)),
            None => Ok(format!("{}", f)),
        },
        FormatType::Scientific => match spec.precision {
            Some(prec) => Ok(format!("{:.prec$e}", f, prec = prec)),
            None => Ok(format!("{:e}", f)),
        },
        FormatType::Decimal => Ok(format!("{}", f as i64)),
        FormatType::Hex => Ok(format!("{:x}", f as i64)),
        FormatType::HexUpper => Ok(format!("{:X}", f as i64)),
        FormatType::Octal => Ok(format!("{:o}", f as i64)),
        FormatType::Binary => Ok(format!("{:b}", f as i64)),
        _ => Err((
            SIG_ERROR,
            error_val(
                "format-error",
                format!(
                    "string/format: cannot format float with spec '{}'",
                    spec_type_char(spec.ty)
                ),
            ),
        )),
    }
}

fn format_string(s: &str, spec: &FormatSpec) -> Result<String, (SignalBits, Value)> {
    match spec.ty {
        FormatType::None | FormatType::StringType => {
            if let Some(prec) = spec.precision {
                Ok(s.chars().take(prec).collect())
            } else {
                Ok(s.to_string())
            }
        }
        _ => Err((
            SIG_ERROR,
            error_val(
                "format-error",
                format!(
                    "string/format: cannot format string with spec '{}'",
                    spec_type_char(spec.ty)
                ),
            ),
        )),
    }
}

/// Return the type character for error messages.
fn spec_type_char(ty: FormatType) -> &'static str {
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

// ============================================================================
// Width and alignment
// ============================================================================

fn apply_width_align(s: &str, spec: &FormatSpec) -> Result<String, (SignalBits, Value)> {
    let width = match spec.width {
        Some(w) => w,
        None => return Ok(s.to_string()),
    };

    let char_count = s.chars().count();
    if char_count >= width {
        return Ok(s.to_string());
    }

    let padding = width - char_count;
    let fill = spec.fill;

    // Align::Default is resolved in format_value before reaching here.
    let (left_pad, right_pad) = match spec.align {
        Align::Left => (0, padding),
        Align::Right => (padding, 0),
        Align::Center => {
            let left = padding / 2;
            let right = padding - left;
            (left, right)
        }
        Align::Default => unreachable!(),
    };

    let mut result = String::with_capacity(width);
    for _ in 0..left_pad {
        result.push(fill);
    }
    result.push_str(s);
    for _ in 0..right_pad {
        result.push(fill);
    }

    Ok(result)
}

// ============================================================================
// Template reconstruction (handles escaped braces)
// ============================================================================

/// Build the output string by replacing placeholders with formatted values.
///
/// Handles `{{` → `{` and `}}` → `}` escape sequences in the literal
/// segments between placeholders.
fn build_output(template: &str, placeholders: &[Placeholder<'_>], formatted: &[String]) -> String {
    let mut result = String::new();
    let mut last_end = 0;

    for (i, ph) in placeholders.iter().enumerate() {
        // Append literal segment, unescaping `{{` and `}}`
        unescape_into(&mut result, &template[last_end..ph.start]);
        result.push_str(&formatted[i]);
        last_end = ph.end;
    }

    // Append trailing literal segment
    unescape_into(&mut result, &template[last_end..]);
    result
}

/// Append `segment` to `out`, replacing `{{` with `{` and `}}` with `}`.
fn unescape_into(out: &mut String, segment: &str) {
    let bytes = segment.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            out.push('{');
            i += 2;
        } else if bytes[i] == b'}' && i + 1 < bytes.len() && bytes[i + 1] == b'}' {
            out.push('}');
            i += 2;
        } else {
            // SAFETY: the original string is valid UTF-8, and we only split
            // on ASCII bytes (`{`, `}`), so each remaining byte is part of a
            // valid UTF-8 sequence.  Push the full char.
            let ch = segment[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
}

// ============================================================================
// Mode dispatch
// ============================================================================

fn format_positional(
    template: &str,
    placeholders: &[Placeholder<'_>],
    args: &[Value],
) -> (SignalBits, Value) {
    if args.len() != placeholders.len() {
        return (
            SIG_ERROR,
            error_val(
                "format-error",
                format!(
                    "string/format: expected {} arguments, got {}",
                    placeholders.len(),
                    args.len()
                ),
            ),
        );
    }

    let mut formatted = Vec::with_capacity(placeholders.len());
    for (i, ph) in placeholders.iter().enumerate() {
        match format_value(&args[i], ph.spec) {
            Ok(s) => formatted.push(s),
            Err(e) => return e,
        }
    }

    let result = build_output(template, placeholders, &formatted);
    (SIG_OK, Value::string(result))
}

fn format_named(
    template: &str,
    placeholders: &[Placeholder<'_>],
    args: &[Value],
) -> (SignalBits, Value) {
    // Must have even number of args (key-value pairs)
    if !args.len().is_multiple_of(2) {
        return (
            SIG_ERROR,
            error_val(
                "format-error",
                "string/format: odd number of keyword arguments",
            ),
        );
    }

    // Build keyword map
    use std::collections::HashMap;
    let mut kwargs: HashMap<&str, Value> = HashMap::new();
    let mut provided_keys: Vec<&str> = Vec::new();
    for i in (0..args.len()).step_by(2) {
        let key = match args[i].as_keyword_name() {
            Some(name) => name,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "string/format: expected keyword, got {}",
                            args[i].type_name()
                        ),
                    ),
                );
            }
        };
        kwargs.insert(key, args[i + 1]);
        provided_keys.push(key);
    }

    // Check all placeholders have values
    for ph in placeholders {
        if !kwargs.contains_key(ph.name) {
            return (
                SIG_ERROR,
                error_val(
                    "format-error",
                    format!("string/format: missing key '{}'", ph.name),
                ),
            );
        }
    }

    // Check no extra keys (keys provided but not used by any placeholder)
    use std::collections::HashSet;
    let used_keys: HashSet<&str> = placeholders.iter().map(|p| p.name).collect();
    for key in &provided_keys {
        if !used_keys.contains(key) {
            return (
                SIG_ERROR,
                error_val(
                    "format-error",
                    format!("string/format: unexpected key '{}'", key),
                ),
            );
        }
    }

    // Format each placeholder
    let mut formatted = Vec::with_capacity(placeholders.len());
    for ph in placeholders {
        let value = kwargs[ph.name];
        match format_value(&value, ph.spec) {
            Ok(s) => formatted.push(s),
            Err(e) => return e,
        }
    }

    let result = build_output(template, placeholders, &formatted);
    (SIG_OK, Value::string(result))
}

// ============================================================================
// Entry point
// ============================================================================

pub fn prim_string_format(args: &[Value]) -> (SignalBits, Value) {
    // Template is the first argument — arity enforced by VM (AtLeast(1))
    let template = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "string/format: template must be string, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    // Parse placeholders
    let placeholders = match parse_placeholders(&template) {
        Ok(p) => p,
        Err(e) => return e,
    };

    // No placeholders: return template as-is (with brace unescaping)
    if placeholders.is_empty() {
        let mut result = String::new();
        unescape_into(&mut result, &template);
        return (SIG_OK, Value::string(result));
    }

    // Determine mode: positional vs named
    let has_named = placeholders.iter().any(|p| !p.name.is_empty());
    let has_positional = placeholders.iter().any(|p| p.name.is_empty());

    if has_named && has_positional {
        return (
            SIG_ERROR,
            error_val(
                "format-error",
                "string/format: cannot mix positional and named arguments",
            ),
        );
    }

    if has_named {
        format_named(&template, &placeholders, &args[1..])
    } else {
        format_positional(&template, &placeholders, &args[1..])
    }
}

// ============================================================================
// Registration
// ============================================================================

pub const PRIMITIVES: &[PrimitiveDef] = &[PrimitiveDef {
    name: "string/format",
    func: prim_string_format,
    effect: Effect::none(),
    arity: Arity::AtLeast(1),
    doc: "Format a template string with positional or named arguments.",
    params: &["template", "args"],
    category: "string",
    example: "(string/format \"{} + {} = {}\" 1 2 3) #=> \"1 + 2 = 3\"",
    aliases: &[],
}];
