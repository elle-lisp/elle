//! Introspection primitives: messages, fields, enums.

use std::collections::BTreeMap;

use prost_reflect::Kind;

use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::{error_val, TableKey, Value};

use crate::schema::get_pool;

// ---------------------------------------------------------------------------
// protobuf/messages
// ---------------------------------------------------------------------------

/// `(protobuf/messages pool)`
///
/// Returns an immutable array of fully-qualified message names.
pub fn prim_messages(args: &[Value]) -> (SignalBits, Value) {
    const PRIM: &str = "protobuf/messages";

    let pool = match get_pool(&args[0], PRIM) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let names: Vec<Value> = pool
        .all_messages()
        .map(|desc| Value::string(desc.full_name()))
        .collect();

    (SIG_OK, Value::array(names))
}

// ---------------------------------------------------------------------------
// protobuf/fields
// ---------------------------------------------------------------------------

/// `(protobuf/fields pool "MessageName")`
///
/// Returns an immutable array of field descriptor structs.
pub fn prim_fields(args: &[Value]) -> (SignalBits, Value) {
    const PRIM: &str = "protobuf/fields";

    let pool = match get_pool(&args[0], PRIM) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let msg_name = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: message name must be a string, got {}",
                        PRIM,
                        args[1].type_name()
                    ),
                ),
            );
        }
    };

    let msg_desc = match pool.get_message_by_name(&msg_name) {
        Some(d) => d,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "protobuf-error",
                    format!("{}: message '{}' not found in pool", PRIM, msg_name),
                ),
            );
        }
    };

    let field_structs: Vec<Value> = msg_desc
        .fields()
        .map(|f| {
            let mut map: BTreeMap<TableKey, Value> = BTreeMap::new();

            // :name — string
            map.insert(TableKey::Keyword("name".into()), Value::string(f.name()));

            // :number — int
            map.insert(
                TableKey::Keyword("number".into()),
                Value::int(f.number() as i64),
            );

            // :type — keyword
            let type_kw = kind_to_keyword(&f.kind());
            map.insert(TableKey::Keyword("type".into()), Value::keyword(type_kw));

            // :label — keyword
            // Note: proto3 has no required fields; prost-reflect 0.14 does not
            // expose is_required(). We report :repeated or :optional only.
            let label_kw = if f.is_list() { "repeated" } else { "optional" };
            map.insert(TableKey::Keyword("label".into()), Value::keyword(label_kw));

            // :message-type — present only for message, enum, and map fields
            let message_type_opt = match &f.kind() {
                Kind::Message(msg) => Some(msg.full_name().to_string()),
                Kind::Enum(e) => Some(e.full_name().to_string()),
                _ => None,
            };
            if let Some(mt) = message_type_opt {
                map.insert(
                    TableKey::Keyword("message-type".into()),
                    Value::string(mt.as_str()),
                );
            }

            Value::struct_from(map)
        })
        .collect();

    (SIG_OK, Value::array(field_structs))
}

// ---------------------------------------------------------------------------
// protobuf/enums
// ---------------------------------------------------------------------------

/// `(protobuf/enums pool)`
///
/// Returns an immutable array of enum descriptor structs.
pub fn prim_enums(args: &[Value]) -> (SignalBits, Value) {
    const PRIM: &str = "protobuf/enums";

    let pool = match get_pool(&args[0], PRIM) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let enum_structs: Vec<Value> = pool
        .all_enums()
        .map(|e| {
            let mut map: BTreeMap<TableKey, Value> = BTreeMap::new();

            // :name — string (fully qualified)
            map.insert(
                TableKey::Keyword("name".into()),
                Value::string(e.full_name()),
            );

            // :values — array of {:name "FOO" :number 0}
            let values: Vec<Value> = e
                .values()
                .map(|v| {
                    let mut vmap: BTreeMap<TableKey, Value> = BTreeMap::new();
                    vmap.insert(TableKey::Keyword("name".into()), Value::string(v.name()));
                    vmap.insert(
                        TableKey::Keyword("number".into()),
                        Value::int(v.number() as i64),
                    );
                    Value::struct_from(vmap)
                })
                .collect();

            map.insert(TableKey::Keyword("values".into()), Value::array(values));

            Value::struct_from(map)
        })
        .collect();

    (SIG_OK, Value::array(enum_structs))
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn kind_to_keyword(kind: &Kind) -> &'static str {
    match kind {
        Kind::Double => "double",
        Kind::Float => "float",
        Kind::Int32 => "int32",
        Kind::Int64 => "int64",
        Kind::Uint32 => "uint32",
        Kind::Uint64 => "uint64",
        Kind::Sint32 => "sint32",
        Kind::Sint64 => "sint64",
        Kind::Fixed32 => "fixed32",
        Kind::Fixed64 => "fixed64",
        Kind::Sfixed32 => "sfixed32",
        Kind::Sfixed64 => "sfixed64",
        Kind::Bool => "bool",
        Kind::String => "string",
        Kind::Bytes => "bytes",
        Kind::Message(_) => "message",
        Kind::Enum(_) => "enum",
    }
}
