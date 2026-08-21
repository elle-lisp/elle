//! Template reconstruction: splice formatted values into literal segments,
//! unescaping `{{`/`}}` as we go.
use super::parse::Placeholder;

/// Build the output string by replacing placeholders with formatted values.
///
/// Handles `{{` → `{` and `}}` → `}` escape sequences in the literal
/// segments between placeholders.
pub(super) fn build_output(
    template: &str,
    placeholders: &[Placeholder<'_>],
    formatted: &[String],
) -> String {
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
pub(super) fn unescape_into(out: &mut String, segment: &str) {
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
