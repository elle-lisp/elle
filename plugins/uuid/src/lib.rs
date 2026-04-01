//! Elle uuid plugin — UUID generation and parsing via the `uuid` crate.

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, Value};
use uuid::Uuid;

/// Plugin entry point. Called by Elle when loading the `.so`.
#[no_mangle]
/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`. The caller must pass a valid
/// `PluginContext` reference. Only safe when called from `load_plugin`.
elle::elle_plugin_init!(PRIMITIVES, "uuid/");

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_uuid_v4(_args: &[Value]) -> (SignalBits, Value) {
    let id = Uuid::new_v4();
    (SIG_OK, Value::string(id.to_string().as_str()))
}

fn prim_uuid_v5(args: &[Value]) -> (SignalBits, Value) {
    let ns_str = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "uuid/v5: expected string for namespace, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let name_str = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "uuid/v5: expected string for name, got {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    };
    let namespace = match Uuid::parse_str(&ns_str) {
        Ok(u) => u,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val(
                    "uuid-error",
                    format!("uuid/v5: invalid namespace UUID: {}", e),
                ),
            );
        }
    };
    let id = Uuid::new_v5(&namespace, name_str.as_bytes());
    (SIG_OK, Value::string(id.to_string().as_str()))
}

fn prim_uuid_parse(args: &[Value]) -> (SignalBits, Value) {
    let s = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("uuid/parse: expected string, got {}", args[0].type_name()),
                ),
            );
        }
    };
    match Uuid::parse_str(&s) {
        Ok(u) => (SIG_OK, Value::string(u.to_string().as_str())),
        Err(e) => (
            SIG_ERROR,
            error_val("uuid-error", format!("uuid/parse: {}", e)),
        ),
    }
}

fn prim_uuid_nil(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::string(Uuid::nil().to_string().as_str()))
}

fn prim_uuid_version(args: &[Value]) -> (SignalBits, Value) {
    let s = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("uuid/version: expected string, got {}", args[0].type_name()),
                ),
            );
        }
    };
    let u = match Uuid::parse_str(&s) {
        Ok(u) => u,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("uuid-error", format!("uuid/version: {}", e)),
            );
        }
    };
    match u.get_version_num() {
        0 => (SIG_OK, Value::NIL),
        n => (SIG_OK, Value::int(n as i64)),
    }
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "uuid/v4",
        func: prim_uuid_v4,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Generate a random UUID (version 4)",
        params: &[],
        category: "uuid",
        example: "(uuid/v4)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "uuid/v5",
        func: prim_uuid_v5,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Generate a deterministic UUID (version 5) from a namespace UUID and a name",
        params: &["namespace", "name"],
        category: "uuid",
        example: r#"(uuid/v5 "6ba7b810-9dad-11d1-80b4-00c04fd430c8" "example.com")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "uuid/parse",
        func: prim_uuid_parse,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse and normalize a UUID string to lowercase hyphenated form",
        params: &["s"],
        category: "uuid",
        example: r#"(uuid/parse "550E8400-E29B-41D4-A716-446655440000")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "uuid/nil",
        func: prim_uuid_nil,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Return the nil UUID (all zeros)",
        params: &[],
        category: "uuid",
        example: "(uuid/nil)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "uuid/version",
        func: prim_uuid_version,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the version number of a UUID string as an integer, or nil if unrecognized",
        params: &["uuid-str"],
        category: "uuid",
        example: "(uuid/version (uuid/v4))",
        aliases: &[],
    },
];
