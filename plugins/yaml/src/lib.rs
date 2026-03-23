//! Elle YAML plugin — YAML parsing and serialization via the `serde_yaml_ng` crate.

use std::collections::BTreeMap;

use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::{Arity, TableKey};
use elle::value::{error_val, Value};
use serde::Deserialize;

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
        let short_name = def.name.strip_prefix("yaml/").unwrap_or(def.name);
        fields.insert(
            TableKey::Keyword(short_name.into()),
            Value::native_fn(def.func),
        );
    }
    Value::struct_from(fields)
}

// ---------------------------------------------------------------------------
// Type conversion: YAML → Elle
// ---------------------------------------------------------------------------

/// Recursively convert a `serde_yaml_ng::Value` to an Elle `Value`.
/// Mappings become immutable structs with keyword keys.
/// Sequences become immutable arrays.
/// Null becomes `Value::NIL`.
fn yaml_to_value(yv: serde_yaml_ng::Value) -> Result<Value, String> {
    match yv {
        serde_yaml_ng::Value::Null => Ok(Value::NIL),
        serde_yaml_ng::Value::Bool(b) => Ok(Value::bool(b)),
        serde_yaml_ng::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::int(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::float(f))
            } else {
                Err(format!("yaml: unsupported number: {}", n))
            }
        }
        serde_yaml_ng::Value::String(s) => Ok(Value::string(s)),
        serde_yaml_ng::Value::Sequence(seq) => {
            let items: Result<Vec<_>, _> = seq.into_iter().map(yaml_to_value).collect();
            Ok(Value::array(items?))
        }
        serde_yaml_ng::Value::Mapping(map) => {
            let mut fields = BTreeMap::new();
            for (k, v) in map {
                let key = match k {
                    serde_yaml_ng::Value::String(s) => s,
                    other => {
                        return Err(format!("yaml: non-string map key: {:?}", other));
                    }
                };
                fields.insert(TableKey::Keyword(key), yaml_to_value(v)?);
            }
            Ok(Value::struct_from(fields))
        }
        serde_yaml_ng::Value::Tagged(tagged) => yaml_to_value(tagged.value),
    }
}

// ---------------------------------------------------------------------------
// Type conversion: Elle → YAML
// ---------------------------------------------------------------------------

/// Recursively convert an Elle `Value` to a `serde_yaml_ng::Value`.
/// Returns an error for types that have no YAML equivalent (closures, etc.).
/// nil → Null (YAML supports null, unlike TOML).
fn value_to_yaml(v: Value, name: &str) -> Result<serde_yaml_ng::Value, (SignalBits, Value)> {
    if v.is_nil() {
        return Ok(serde_yaml_ng::Value::Null);
    }
    if let Some(b) = v.as_bool() {
        return Ok(serde_yaml_ng::Value::Bool(b));
    }
    if let Some(i) = v.as_int() {
        return Ok(serde_yaml_ng::Value::Number(i.into()));
    }
    if let Some(f) = v.as_float() {
        return Ok(serde_yaml_ng::Value::Number(serde_yaml_ng::Number::from(f)));
    }
    if let Some(s) = v.with_string(|s| s.to_string()) {
        return Ok(serde_yaml_ng::Value::String(s));
    }
    // Immutable array
    if let Some(arr) = v.as_array() {
        let items: Result<Vec<_>, _> = arr.iter().map(|&item| value_to_yaml(item, name)).collect();
        return Ok(serde_yaml_ng::Value::Sequence(items?));
    }
    // Mutable @array
    if let Some(arr_ref) = v.as_array_mut() {
        let arr = arr_ref.borrow();
        let items: Result<Vec<_>, _> = arr.iter().map(|&item| value_to_yaml(item, name)).collect();
        return Ok(serde_yaml_ng::Value::Sequence(items?));
    }
    // Immutable struct — keyword keys become YAML mapping string keys
    if let Some(map) = v.as_struct() {
        let mut mapping = serde_yaml_ng::Mapping::new();
        for (k, &val) in map.iter() {
            let key = match k {
                TableKey::Keyword(s) => serde_yaml_ng::Value::String(s.clone()),
                other => {
                    return Err((
                        SIG_ERROR,
                        error_val(
                            "yaml-error",
                            format!("{}: struct key must be a keyword, got {:?}", name, other),
                        ),
                    ))
                }
            };
            mapping.insert(key, value_to_yaml(val, name)?);
        }
        return Ok(serde_yaml_ng::Value::Mapping(mapping));
    }
    // Mutable @struct — same treatment
    if let Some(map_ref) = v.as_struct_mut() {
        let map = map_ref.borrow();
        let mut mapping = serde_yaml_ng::Mapping::new();
        for (k, &val) in map.iter() {
            let key = match k {
                TableKey::Keyword(s) => serde_yaml_ng::Value::String(s.clone()),
                other => {
                    return Err((
                        SIG_ERROR,
                        error_val(
                            "yaml-error",
                            format!("{}: struct key must be a keyword, got {:?}", name, other),
                        ),
                    ))
                }
            };
            mapping.insert(key, value_to_yaml(val, name)?);
        }
        return Ok(serde_yaml_ng::Value::Mapping(mapping));
    }
    Err((
        SIG_ERROR,
        error_val(
            "yaml-error",
            format!("{}: cannot encode {} as YAML", name, v.type_name()),
        ),
    ))
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_yaml_parse(args: &[Value]) -> (SignalBits, Value) {
    let name = "yaml/parse";
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
    match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text) {
        Ok(yv) => match yaml_to_value(yv) {
            Ok(v) => (SIG_OK, v),
            Err(e) => (
                SIG_ERROR,
                error_val("yaml-error", format!("{}: {}", name, e)),
            ),
        },
        Err(e) => (
            SIG_ERROR,
            error_val("yaml-error", format!("{}: {}", name, e)),
        ),
    }
}

fn prim_yaml_parse_all(args: &[Value]) -> (SignalBits, Value) {
    let name = "yaml/parse-all";
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
    let mut docs = Vec::new();
    for doc in serde_yaml_ng::Deserializer::from_str(&text) {
        let yv = match serde_yaml_ng::Value::deserialize(doc) {
            Ok(v) => v,
            Err(e) => {
                return (
                    SIG_ERROR,
                    error_val("yaml-error", format!("{}: {}", name, e)),
                )
            }
        };
        match yaml_to_value(yv) {
            Ok(v) => docs.push(v),
            Err(e) => {
                return (
                    SIG_ERROR,
                    error_val("yaml-error", format!("{}: {}", name, e)),
                )
            }
        }
    }
    (SIG_OK, Value::array(docs))
}

fn prim_yaml_encode(args: &[Value]) -> (SignalBits, Value) {
    let name = "yaml/encode";
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("{}: expected 1 argument, got {}", name, args.len()),
            ),
        );
    }
    let yv = match value_to_yaml(args[0], name) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match serde_yaml_ng::to_string(&yv) {
        Ok(s) => (SIG_OK, Value::string(s)),
        Err(e) => (
            SIG_ERROR,
            error_val("yaml-error", format!("{}: {}", name, e)),
        ),
    }
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "yaml/parse",
        func: prim_yaml_parse,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse a YAML string (first document) to an Elle value. Mappings become immutable structs with keyword keys. Sequences become immutable arrays. Null becomes nil.",
        params: &["text"],
        category: "yaml",
        example: r#"(yaml/parse "name: hello\nversion: 1")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "yaml/parse-all",
        func: prim_yaml_parse_all,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse all YAML documents in a string. Returns an array of values, one per document. Documents are separated by `---`.",
        params: &["text"],
        category: "yaml",
        example: r#"(yaml/parse-all "---\na: 1\n---\nb: 2")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "yaml/encode",
        func: prim_yaml_encode,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Encode an Elle value to a YAML string. Structs become YAML mappings. Arrays become YAML sequences. nil becomes YAML null.",
        params: &["value"],
        category: "yaml",
        example: r#"(yaml/encode {:name "hello" :version 1})"#,
        aliases: &[],
    },
];
