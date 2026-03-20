//! Elle regex plugin — regular expression support via the `regex` crate.

use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
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
pub unsafe extern "C" fn elle_plugin_init(ctx: &mut PluginContext) -> Value {
    let mut fields = BTreeMap::new();
    for def in PRIMITIVES {
        ctx.register(def);
        let short_name = def.name.strip_prefix("regex/").unwrap_or(def.name);
        fields.insert(
            TableKey::Keyword(short_name.into()),
            Value::native_fn(def.func),
        );
    }
    Value::struct_from(fields)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn require_arity(name: &str, args: &[Value], expected: usize) -> Result<(), (SignalBits, Value)> {
    if args.len() != expected {
        Err((
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "{}: expected {} argument{}, got {}",
                    name,
                    expected,
                    if expected == 1 { "" } else { "s" },
                    args.len()
                ),
            ),
        ))
    } else {
        Ok(())
    }
}

fn require_regex<'a>(name: &str, v: &'a Value) -> Result<&'a Regex, (SignalBits, Value)> {
    v.as_external::<Regex>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected regex, got {}", name, v.type_name()),
            ),
        )
    })
}

fn require_string(name: &str, v: &Value) -> Result<String, (SignalBits, Value)> {
    v.with_string(|s| s.to_string()).ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected string, got {}", name, v.type_name()),
            ),
        )
    })
}

fn match_struct(m: regex::Match<'_>) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(TableKey::Keyword("match".into()), Value::string(m.as_str()));
    fields.insert(
        TableKey::Keyword("start".into()),
        Value::int(m.start() as i64),
    );
    fields.insert(TableKey::Keyword("end".into()), Value::int(m.end() as i64));
    Value::struct_from(fields)
}

fn captures_struct(re: &Regex, caps: &regex::Captures<'_>) -> Value {
    let mut fields = BTreeMap::new();
    for (i, m) in caps.iter().enumerate() {
        if let Some(m) = m {
            let key = TableKey::Keyword(format!("{}", i));
            fields.insert(key, Value::string(m.as_str()));
        }
    }
    for name in re.capture_names().flatten() {
        if let Some(m) = caps.name(name) {
            let key = TableKey::Keyword(name.to_string());
            fields.insert(key, Value::string(m.as_str()));
        }
    }
    Value::struct_from(fields)
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_regex_compile(args: &[Value]) -> (SignalBits, Value) {
    if let Err(e) = require_arity("regex/compile", args, 1) {
        return e;
    }
    let pattern = match require_string("regex/compile", &args[0]) {
        Ok(s) => s,
        Err(e) => return e,
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
    if let Err(e) = require_arity("regex/match?", args, 2) {
        return e;
    }
    let re = match require_regex("regex/match?", &args[0]) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let text = match require_string("regex/match?", &args[1]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    (SIG_OK, Value::bool(re.is_match(&text)))
}

fn prim_regex_find(args: &[Value]) -> (SignalBits, Value) {
    if let Err(e) = require_arity("regex/find", args, 2) {
        return e;
    }
    let re = match require_regex("regex/find", &args[0]) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let text = match require_string("regex/find", &args[1]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match re.find(&text) {
        Some(m) => (SIG_OK, match_struct(m)),
        None => (SIG_OK, Value::NIL),
    }
}

fn prim_regex_find_all(args: &[Value]) -> (SignalBits, Value) {
    if let Err(e) = require_arity("regex/find-all", args, 2) {
        return e;
    }
    let re = match require_regex("regex/find-all", &args[0]) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let text = match require_string("regex/find-all", &args[1]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let matches: Vec<Value> = re.find_iter(&text).map(match_struct).collect();
    (SIG_OK, elle::list(matches))
}

fn prim_regex_captures(args: &[Value]) -> (SignalBits, Value) {
    if let Err(e) = require_arity("regex/captures", args, 2) {
        return e;
    }
    let re = match require_regex("regex/captures", &args[0]) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let text = match require_string("regex/captures", &args[1]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match re.captures(&text) {
        Some(caps) => (SIG_OK, captures_struct(re, &caps)),
        None => (SIG_OK, Value::NIL),
    }
}

fn prim_regex_captures_all(args: &[Value]) -> (SignalBits, Value) {
    if let Err(e) = require_arity("regex/captures-all", args, 2) {
        return e;
    }
    let re = match require_regex("regex/captures-all", &args[0]) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let text = match require_string("regex/captures-all", &args[1]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let results: Vec<Value> = re
        .captures_iter(&text)
        .map(|caps| captures_struct(re, &caps))
        .collect();
    (SIG_OK, elle::list(results))
}

fn prim_regex_replace(args: &[Value]) -> (SignalBits, Value) {
    if let Err(e) = require_arity("regex/replace", args, 3) {
        return e;
    }
    let re = match require_regex("regex/replace", &args[0]) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let text = match require_string("regex/replace", &args[1]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let replacement = match require_string("regex/replace", &args[2]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    (
        SIG_OK,
        Value::string(re.replace(&text, replacement.as_str())),
    )
}

fn prim_regex_replace_all(args: &[Value]) -> (SignalBits, Value) {
    if let Err(e) = require_arity("regex/replace-all", args, 3) {
        return e;
    }
    let re = match require_regex("regex/replace-all", &args[0]) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let text = match require_string("regex/replace-all", &args[1]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let replacement = match require_string("regex/replace-all", &args[2]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    (
        SIG_OK,
        Value::string(re.replace_all(&text, replacement.as_str())),
    )
}

fn prim_regex_split(args: &[Value]) -> (SignalBits, Value) {
    if let Err(e) = require_arity("regex/split", args, 2) {
        return e;
    }
    let re = match require_regex("regex/split", &args[0]) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let text = match require_string("regex/split", &args[1]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let parts: Vec<Value> = re.split(&text).map(Value::string).collect();
    (SIG_OK, elle::list(parts))
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "regex/compile",
        func: prim_regex_compile,
        signal: Signal::errors(),
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
        signal: Signal::errors(),
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
        signal: Signal::errors(),
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
        signal: Signal::errors(),
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
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Capture groups from first match. Returns a struct with numbered and named groups, or nil.",
        params: &["regex", "text"],
        category: "regex",
        example: r#"(regex/captures (regex/compile "(?P<year>\\d{4})-(?P<month>\\d{2})") "2024-01-15")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "regex/captures-all",
        func: prim_regex_captures_all,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Capture groups from all matches. Returns a list of capture structs.",
        params: &["regex", "text"],
        category: "regex",
        example: r#"(regex/captures-all (regex/compile "(\\d+)-(\\w+)") "1-a 2-b 3-c")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "regex/replace",
        func: prim_regex_replace,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Replace the first match in text. Supports $1, $name backreferences in replacement.",
        params: &["regex", "text", "replacement"],
        category: "regex",
        example: r#"(regex/replace (regex/compile "\\d+") "a1b2" "N")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "regex/replace-all",
        func: prim_regex_replace_all,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Replace all matches in text. Supports $1, $name backreferences in replacement.",
        params: &["regex", "text", "replacement"],
        category: "regex",
        example: r#"(regex/replace-all (regex/compile "\\d+") "a1b2" "N")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "regex/split",
        func: prim_regex_split,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Split a string by regex pattern. Returns a list of strings.",
        params: &["regex", "text"],
        category: "regex",
        example: r#"(regex/split (regex/compile "[,;\\s]+") "a,b; c  d")"#,
        aliases: &[],
    },
];
