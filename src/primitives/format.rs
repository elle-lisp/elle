// audited: 2026-11-20
//! The `string/format` primitive: it parses the template, then substitutes positional or named arguments.
//!
//! src/primitives/format/AGENTS.md

use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

mod build;
mod dispatch;
mod parse;
mod value;

use build::unescape_into;
use dispatch::{format_named, format_positional};
use parse::parse_placeholders;

pub(crate) fn prim_string_format(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Template is the first argument — arity enforced by VM (AtLeast(1))
    let template = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
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
    let placeholders = match parse_placeholders(&template, ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };

    // No placeholders: return template as-is (with brace unescaping)
    if placeholders.is_empty() {
        let mut result = String::new();
        unescape_into(&mut result, &template);
        return (SIG_OK, ctx.string(result));
    }

    // Determine mode: positional vs named
    let has_named = placeholders.iter().any(|p| !p.name.is_empty());
    let has_positional = placeholders.iter().any(|p| p.name.is_empty());

    if has_named && has_positional {
        return (
            SIG_ERROR,
            ctx.error(
                "format-error",
                "string/format: cannot mix positional and named arguments",
            ),
        );
    }

    if has_named {
        format_named(ctx, &template, &placeholders, &args[1..])
    } else {
        format_positional(ctx, &template, &placeholders, &args[1..])
    }
}

// ============================================================================
// Registration
// ============================================================================

primitive! {
    "string/format" => prim_string_format {
        signal: Signal::errors(),
        arity: Arity::AtLeast(1),
        doc: "Format a template string with positional or named arguments. A placeholder \
              with no type renders a number or a string as itself, and any other value as \
              `string` renders it.",
        params: &["template", "args"],
        category: "string",
        example: "(string/format \"{} + {} = {}\" 1 2 3) #=> \"1 + 2 = 3\"",
        effect: RegionEffect::Fresh,
    }
}
