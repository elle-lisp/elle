//! Mode dispatch: match placeholders against positional or named arguments.
use super::build::build_output;
use super::parse::Placeholder;
use super::value::format_value;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::Value;

pub(super) fn format_positional(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    template: &str,
    placeholders: &[Placeholder<'_>],
    args: &[Value],
) -> (SignalBits, Value) {
    if args.len() != placeholders.len() {
        return (
            SIG_ERROR,
            ctx.error(
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
        match format_value(&args[i], ph.spec, ctx) {
            Ok(s) => formatted.push(s),
            Err(e) => return e,
        }
    }

    let result = build_output(template, placeholders, &formatted);
    (SIG_OK, ctx.string(result))
}

pub(super) fn format_named(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    template: &str,
    placeholders: &[Placeholder<'_>],
    args: &[Value],
) -> (SignalBits, Value) {
    // Must have even number of args (key-value pairs)
    if !args.len().is_multiple_of(2) {
        return (
            SIG_ERROR,
            ctx.error(
                "format-error",
                "string/format: odd number of keyword arguments",
            ),
        );
    }

    // Build keyword map
    use std::collections::HashMap;
    let mut kwargs: HashMap<String, Value> = HashMap::new();
    let mut provided_keys: Vec<String> = Vec::new();
    for i in (0..args.len()).step_by(2) {
        let key = match args[i].as_keyword_name() {
            Some(name) => name,
            None => {
                return type_error!(ctx, args[i], "string/format", "keyword");
            }
        };
        kwargs.insert(key.clone(), args[i + 1]);
        provided_keys.push(key);
    }

    // Check all placeholders have values
    for ph in placeholders {
        if !kwargs.contains_key(ph.name) {
            return (
                SIG_ERROR,
                ctx.error(
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
        if !used_keys.contains(key.as_str()) {
            return (
                SIG_ERROR,
                ctx.error(
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
        match format_value(&value, ph.spec, ctx) {
            Ok(s) => formatted.push(s),
            Err(e) => return e,
        }
    }

    let result = build_output(template, placeholders, &formatted);
    (SIG_OK, ctx.string(result))
}
