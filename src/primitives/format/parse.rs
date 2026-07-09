//! Template parsing: split the template into literal segments and placeholders.
use crate::primitives::ctx::NativeCtx;
use crate::value::fiber::{SignalBits, SIG_ERROR};
use crate::value::Value;

/// A parsed placeholder from the template string.
pub(super) struct Placeholder<'a> {
    /// Name of the placeholder (empty string for positional `{}`).
    pub(super) name: &'a str,
    /// Raw format spec string (everything after `:`, empty if no spec).
    pub(super) spec: &'a str,
    /// Byte offset of the opening `{` in the template.
    pub(super) start: usize,
    /// Byte offset one past the closing `}` in the template.
    pub(super) end: usize,
}

/// Parse template string into literal segments and placeholders.
///
/// Handles `{{` as escaped `{` and `}}` as escaped `}`.
/// Returns a list of placeholders with their byte positions.
pub(super) fn parse_placeholders<'a>(
    template: &'a str,
    ctx: &mut NativeCtx,
) -> Result<Vec<Placeholder<'a>>, (SignalBits, Value)> {
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
                    ctx.error("format-error", "string/format: unmatched '{' in template"),
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
                ctx.error("format-error", "string/format: unmatched '}' in template"),
            ));
        } else {
            i += 1;
        }
    }

    Ok(placeholders)
}
