//! Elle regex plugin — regular expression support via the `regex` crate.

use elle::effects::Effect;
use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};
use regex::Regex;
use std::collections::BTreeMap;

/// Plugin entry point. Called by Elle when loading the `.so`.
#[no_mangle]
/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`. The caller must pass a valid
/// `PluginContext` reference. Only safe when called from `load_plugin`.
pub unsafe extern "C" fn elle_plugin_init(ctx: &mut PluginContext) {
    for def in PRIMITIVES {
        ctx.register(def);
    }
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_regex_compile(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("regex/compile: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let pattern = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "regex/compile: expected string, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    match Regex::new(&pattern) {
        Ok(re) => (SIG_OK, Value::external("regex", re)),
        Err(e) => (
            SIG_ERROR,
            error_val("regex-error", format!("regex/compile: {}", e)),
        ),
    }
}

fn prim_regex_match(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("regex/match?: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let re = match args[0].as_external::<Regex>() {
        Some(r) => r,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("regex/match?: expected regex, got {}", args[0].type_name()),
                ),
            );
        }
    };
    let text = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("regex/match?: expected string, got {}", args[1].type_name()),
                ),
            );
        }
    };
    (SIG_OK, Value::bool(re.is_match(&text)))
}

fn prim_regex_find(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("regex/find: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let re = match args[0].as_external::<Regex>() {
        Some(r) => r,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("regex/find: expected regex, got {}", args[0].type_name()),
                ),
            );
        }
    };
    let text = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("regex/find: expected string, got {}", args[1].type_name()),
                ),
            );
        }
    };
    match re.find(&text) {
        Some(m) => {
            let mut fields = BTreeMap::new();
            fields.insert(TableKey::Keyword("match".into()), Value::string(m.as_str()));
            fields.insert(
                TableKey::Keyword("start".into()),
                Value::int(m.start() as i64),
            );
            fields.insert(TableKey::Keyword("end".into()), Value::int(m.end() as i64));
            (SIG_OK, Value::struct_from(fields))
        }
        None => (SIG_OK, Value::NIL),
    }
}

fn prim_regex_find_all(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("regex/find-all: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let re = match args[0].as_external::<Regex>() {
        Some(r) => r,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "regex/find-all: expected regex, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let text = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "regex/find-all: expected string, got {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    };
    let matches: Vec<Value> = re
        .find_iter(&text)
        .map(|m| {
            let mut fields = BTreeMap::new();
            fields.insert(TableKey::Keyword("match".into()), Value::string(m.as_str()));
            fields.insert(
                TableKey::Keyword("start".into()),
                Value::int(m.start() as i64),
            );
            fields.insert(TableKey::Keyword("end".into()), Value::int(m.end() as i64));
            Value::struct_from(fields)
        })
        .collect();
    (SIG_OK, elle::list(matches))
}

fn prim_regex_captures(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("regex/captures: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let re = match args[0].as_external::<Regex>() {
        Some(r) => r,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "regex/captures: expected regex, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let text = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "regex/captures: expected string, got {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    };
    match re.captures(&text) {
        Some(caps) => {
            let mut fields = BTreeMap::new();
            // Numbered captures
            for (i, m) in caps.iter().enumerate() {
                if let Some(m) = m {
                    let key = TableKey::Keyword(format!("{}", i));
                    fields.insert(key, Value::string(m.as_str()));
                }
            }
            // Named captures
            for name in re.capture_names().flatten() {
                if let Some(m) = caps.name(name) {
                    let key = TableKey::Keyword(name.to_string());
                    fields.insert(key, Value::string(m.as_str()));
                }
            }
            (SIG_OK, Value::struct_from(fields))
        }
        None => (SIG_OK, Value::NIL),
    }
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "regex/compile",
        func: prim_regex_compile,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Compile a regular expression pattern",
        params: &["pattern"],
        category: "regex",
        example: r#"(regex/compile "\\d+")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "regex/match?",
        func: prim_regex_match,
        effect: Effect::raises(),
        arity: Arity::Exact(2),
        doc: "Test if a regex matches a string",
        params: &["regex", "text"],
        category: "regex",
        example: r#"(regex/match? (regex/compile "\\d+") "abc123")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "regex/find",
        func: prim_regex_find,
        effect: Effect::raises(),
        arity: Arity::Exact(2),
        doc: "Find the first match in a string. Returns a struct with :match, :start, :end or nil.",
        params: &["regex", "text"],
        category: "regex",
        example: r#"(regex/find (regex/compile "\\d+") "abc123def")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "regex/find-all",
        func: prim_regex_find_all,
        effect: Effect::raises(),
        arity: Arity::Exact(2),
        doc: "Find all matches in a string. Returns a list of match structs.",
        params: &["regex", "text"],
        category: "regex",
        example: r#"(regex/find-all (regex/compile "\\d+") "a1b2c3")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "regex/captures",
        func: prim_regex_captures,
        effect: Effect::raises(),
        arity: Arity::Exact(2),
        doc: "Capture groups from first match. Returns a struct with numbered and named groups, or nil.",
        params: &["regex", "text"],
        category: "regex",
        example: r#"(regex/captures (regex/compile "(?P<year>\\d{4})-(?P<month>\\d{2})") "2024-01-15")"#,
        aliases: &[],
    },
];
