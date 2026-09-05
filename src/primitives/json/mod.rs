//! JSON parsing and serialization primitives
//!
//! Provides hand-written recursive descent JSON parser and serializer.
//! No external JSON libraries - all implemented directly.

mod parser;
mod serializer;

#[cfg(test)]
mod tests;

pub use parser::JsonParser;
pub use serializer::{escape_json_string, serialize_value, serialize_value_pretty};

use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Parse a JSON string into an immutable Elle value: a JSON array becomes an
/// immutable array, a JSON object an immutable struct.
pub(crate) fn prim_json_parse(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // 2 args is never valid (option key without value, or value without key)
    if args.len() == 2 {
        return (
            SIG_ERROR,
            ctx.error(
                "arity-error",
                "json/parse: expected 1 or 3 arguments".to_string(),
            ),
        );
    }

    let json_str = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                "json/parse: expected string argument".to_string(),
            ),
        );
    };

    let use_keyword_keys = if args.len() == 3 {
        let opt_key_ok = args[1].is_keyword_named("keys");
        let opt_val_ok = args[2].is_keyword_named("keyword");
        if opt_key_ok && opt_val_ok {
            true
        } else {
            return (
                SIG_ERROR,
                ctx.error(
                    "argument-error",
                    "json/parse: expected :keys :keyword".to_string(),
                ),
            );
        }
    } else {
        false
    };

    let result = {
        let mut parser = JsonParser::new_with_opts(&json_str, use_keyword_keys, ctx);
        parser.parse()
    };
    match result {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, ctx.error("serde-error", e)),
    }
}

/// Serialize an Elle value to compact JSON
pub(crate) fn prim_json_serialize(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let json_str = match serialize_value(&args[0], ctx.vm().symbols().map(|s| &*s)) {
        Ok(s) => s,
        Err(e) => return (SIG_ERROR, ctx.error("serde-error", e)),
    };
    (SIG_OK, ctx.string(json_str))
}

/// Serialize an Elle value to pretty-printed JSON with 2-space indentation
pub(crate) fn prim_json_serialize_pretty(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let json_str = match serialize_value_pretty(&args[0], ctx.vm().symbols().map(|s| &*s), 0) {
        Ok(s) => s,
        Err(e) => return (SIG_ERROR, ctx.error("serde-error", e)),
    };
    (SIG_OK, ctx.string(json_str))
}

// All three raise `serde-error` — `json/parse` on malformed input, the two
// serializers on a value JSON cannot represent — so all three declare
// `Signal::errors()`. Effect inference copies that declaration into every
// caller, which is what keeps the error catchable by `try` at any call depth.
// `tests/elle/prim-json.lisp` pins the declaration.
primitive! {
    "json/parse" => prim_json_parse {
        signal: Signal::errors(),
        arity: Arity::Range(1, 3),
        doc: "Parse a JSON string into an immutable Elle value: a JSON array becomes an immutable array and a JSON object an immutable struct. Accepts optional :keys :keyword to use keyword keys in parsed structs instead of string keys.",
        params: &["json-string", ":keys", ":keyword"],
        category: "json",
        example: r#"(json/parse "{\"name\": \"Alice\", \"age\": 30}" :keys :keyword)"#,
        aliases: &["json-parse"],
        effect: RegionEffect::Fresh,
    }
    "json/serialize" => prim_json_serialize {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Serialize an Elle value to compact JSON",
        params: &["value"],
        category: "json",
        example: "(json/serialize (@struct :name \"Bob\" :age 25))",
        aliases: &["json-serialize"],
        effect: RegionEffect::Fresh,
    }
    "json/pretty" => prim_json_serialize_pretty {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Serialize an Elle value to pretty-printed JSON with 2-space indentation",
        params: &["value"],
        category: "json",
        example: "(json/pretty (list 1 2 3))",
        aliases: &["json-serialize-pretty"],
        effect: RegionEffect::Fresh,
    }
}

// Tests migrated to tests/elle/prim-json.lisp
