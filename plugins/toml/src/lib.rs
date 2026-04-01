//! Elle TOML plugin — TOML parsing and serialization via the `toml` crate.

use std::collections::BTreeMap;

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::{Arity, TableKey};
use elle::value::{error_val, Value};
elle::elle_plugin_init!(PRIMITIVES, "toml/");

// ---------------------------------------------------------------------------
// Type conversion: TOML → Elle
// ---------------------------------------------------------------------------

/// Recursively convert a `toml::Value` to an Elle `Value`.
/// Tables become immutable structs with keyword keys.
/// Arrays become immutable arrays.
/// Datetimes become their ISO 8601 string representation.
fn toml_to_value(tv: toml::Value) -> Value {
    match tv {
        toml::Value::String(s) => Value::string(s),
        toml::Value::Integer(i) => Value::int(i),
        toml::Value::Float(f) => Value::float(f),
        toml::Value::Boolean(b) => Value::bool(b),
        toml::Value::Array(arr) => Value::array(arr.into_iter().map(toml_to_value).collect()),
        toml::Value::Table(t) => {
            let mut map = BTreeMap::new();
            for (k, v) in t {
                map.insert(TableKey::Keyword(k), toml_to_value(v));
            }
            Value::struct_from(map)
        }
        toml::Value::Datetime(dt) => Value::string(dt.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Type conversion: Elle → TOML
// ---------------------------------------------------------------------------

/// Recursively convert an Elle `Value` to a `toml::Value`.
/// Returns an error for types that have no TOML equivalent (nil, closures, etc.).
fn value_to_toml(v: Value, name: &str) -> Result<toml::Value, (SignalBits, Value)> {
    if let Some(s) = v.with_string(|s| s.to_string()) {
        return Ok(toml::Value::String(s));
    }
    if let Some(i) = v.as_int() {
        return Ok(toml::Value::Integer(i));
    }
    if let Some(f) = v.as_float() {
        return Ok(toml::Value::Float(f));
    }
    if let Some(b) = v.as_bool() {
        return Ok(toml::Value::Boolean(b));
    }
    // Immutable array
    if let Some(arr) = v.as_array() {
        let items: Result<Vec<_>, _> = arr.iter().map(|&item| value_to_toml(item, name)).collect();
        return Ok(toml::Value::Array(items?));
    }
    // Mutable @array
    if let Some(arr_ref) = v.as_array_mut() {
        let arr = arr_ref.borrow();
        let items: Result<Vec<_>, _> = arr.iter().map(|&item| value_to_toml(item, name)).collect();
        return Ok(toml::Value::Array(items?));
    }
    // Immutable struct — keyword keys become TOML table keys
    if let Some(map) = v.as_struct() {
        let mut table = toml::map::Map::new();
        for (k, &val) in map.iter() {
            let key = match k {
                TableKey::Keyword(s) => s.clone(),
                other => {
                    return Err((
                        SIG_ERROR,
                        error_val(
                            "toml-error",
                            format!("{}: struct key must be a keyword, got {:?}", name, other),
                        ),
                    ))
                }
            };
            table.insert(key, value_to_toml(val, name)?);
        }
        return Ok(toml::Value::Table(table));
    }
    // Mutable @struct — same treatment
    if let Some(map_ref) = v.as_struct_mut() {
        let map = map_ref.borrow();
        let mut table = toml::map::Map::new();
        for (k, &val) in map.iter() {
            let key = match k {
                TableKey::Keyword(s) => s.clone(),
                other => {
                    return Err((
                        SIG_ERROR,
                        error_val(
                            "toml-error",
                            format!("{}: struct key must be a keyword, got {:?}", name, other),
                        ),
                    ))
                }
            };
            table.insert(key, value_to_toml(val, name)?);
        }
        return Ok(toml::Value::Table(table));
    }
    // nil → explicit error (TOML has no null type)
    if v.is_nil() {
        return Err((
            SIG_ERROR,
            error_val(
                "toml-error",
                format!(
                    "{}: cannot encode nil as TOML (TOML has no null type)",
                    name
                ),
            ),
        ));
    }
    Err((
        SIG_ERROR,
        error_val(
            "toml-error",
            format!("{}: cannot encode {} as TOML", name, v.type_name()),
        ),
    ))
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_toml_parse(args: &[Value]) -> (SignalBits, Value) {
    let name = "toml/parse";
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("{}: expected 1 argument, got {}", name, args.len()),
            ),
        );
    }
    let text = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{}: expected string, got {}", name, args[0].type_name()),
                ),
            )
        }
    };
    match toml::from_str::<toml::Value>(&text) {
        Ok(tv) => (SIG_OK, toml_to_value(tv)),
        Err(e) => (
            SIG_ERROR,
            error_val("toml-error", format!("{}: {}", name, e)),
        ),
    }
}

fn prim_toml_encode(args: &[Value]) -> (SignalBits, Value) {
    let name = "toml/encode";
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("{}: expected 1 argument, got {}", name, args.len()),
            ),
        );
    }
    let tv = match value_to_toml(args[0], name) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match toml::to_string(&tv) {
        Ok(s) => (SIG_OK, Value::string(s)),
        Err(e) => (
            SIG_ERROR,
            error_val("toml-error", format!("{}: {}", name, e)),
        ),
    }
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "toml/parse",
        func: prim_toml_parse,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse a TOML string to an Elle value. Tables become immutable structs with keyword keys. Arrays become immutable arrays. Datetimes become strings.",
        params: &["text"],
        category: "toml",
        example: r#"(toml/parse "[package]\nname = \"hello\"")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "toml/encode",
        func: prim_toml_encode,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Encode an Elle value to a TOML string. Structs become TOML tables. Arrays become TOML arrays. nil values are an error (TOML has no null type).",
        params: &["value"],
        category: "toml",
        example: r#"(toml/encode {:name "hello" :version 1})"#,
        aliases: &[],
    },
];
