//! Conversion between Elle `Value`s and `prost_reflect::Value`s.
//!
//! Two public entry points:
//!   - `elle_to_pb`: encode an Elle value into a protobuf field value
//!   - `pb_to_elle`: decode a protobuf field value into an Elle value

use std::collections::{BTreeMap, HashMap};

use prost_reflect::{
    DynamicMessage, FieldDescriptor, Kind, MapKey, ReflectMessage, Value as PbValue,
};

use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::{error_val, TableKey, Value};

use crate::schema::struct_keyword_iter;

// ---------------------------------------------------------------------------
// Elle → Protobuf (encode)
// ---------------------------------------------------------------------------

/// Convert an Elle `Value` to a `prost_reflect::Value` for the given field.
///
/// Returns `Err(String)` with a human-readable error message (caller wraps it
/// with the field name and primitive name).
pub fn elle_to_pb(val: Value, field: &FieldDescriptor) -> Result<PbValue, String> {
    // nil means "don't set this field" — callers must check before calling.
    // Reaching here with nil is a programming error in the caller.
    if val.is_nil() {
        return Err(format!(
            "nil is not a valid value for field '{}'",
            field.name()
        ));
    }

    match field.kind() {
        Kind::Bool => match val.as_bool() {
            Some(b) => Ok(PbValue::Bool(b)),
            None => Err(format!("expected bool, got {}", val.type_name())),
        },
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => {
            let n = elle_to_i32(val, field.name())?;
            Ok(PbValue::I32(n))
        }
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => {
            let n = elle_int_val(val)?;
            Ok(PbValue::I64(n))
        }
        Kind::Uint32 | Kind::Fixed32 => {
            let n = elle_to_u32(val, field.name())?;
            Ok(PbValue::U32(n))
        }
        Kind::Uint64 | Kind::Fixed64 => {
            let n = elle_to_u64(val, field.name())?;
            Ok(PbValue::U64(n))
        }
        Kind::Float => {
            let f = val
                .as_float()
                .ok_or_else(|| format!("expected float, got {}", val.type_name()))?;
            Ok(PbValue::F32(f as f32))
        }
        Kind::Double => {
            let f = val
                .as_float()
                .ok_or_else(|| format!("expected float, got {}", val.type_name()))?;
            Ok(PbValue::F64(f))
        }
        Kind::String => {
            let s = elle_to_string(val)?;
            Ok(PbValue::String(s))
        }
        Kind::Bytes => {
            let b = elle_to_bytes(val)?;
            Ok(PbValue::Bytes(b.into()))
        }
        Kind::Enum(enum_desc) => {
            // Accept keyword (by name) or int (by number, for forward compat).
            if let Some(n) = val.as_int() {
                return Ok(PbValue::EnumNumber(n as i32));
            }
            if let Some(kw) = val.as_keyword_name() {
                match enum_desc.get_value_by_name(&kw) {
                    Some(v) => return Ok(PbValue::EnumNumber(v.number())),
                    None => {
                        return Err(format!(
                            "unknown enum value :{} for enum '{}'",
                            kw,
                            enum_desc.full_name()
                        ));
                    }
                }
            }
            Err(format!(
                "expected keyword or int for enum field '{}', got {}",
                field.name(),
                val.type_name()
            ))
        }
        Kind::Message(msg_desc) => {
            // Repeated fields are handled at the call site (encode_repeated).
            // Map fields have a synthetic message descriptor — handle them
            // separately.
            if msg_desc.is_map_entry() {
                Err(format!(
                    "map field '{}' must be encoded via encode_map, not elle_to_pb",
                    field.name()
                ))
            } else {
                let dyn_msg = encode_message(val, &msg_desc)?;
                Ok(PbValue::Message(dyn_msg))
            }
        }
    }
}

/// Encode an Elle struct into a `DynamicMessage` for `msg_desc`.
pub fn encode_message(
    val: Value,
    msg_desc: &prost_reflect::MessageDescriptor,
) -> Result<DynamicMessage, String> {
    let is_struct = val.as_struct().is_some() || val.as_struct_mut().is_some();
    if !is_struct {
        return Err(format!("expected struct, got {}", val.type_name()));
    }

    let mut msg = DynamicMessage::new(msg_desc.clone());

    // Collect keyword key-value pairs from the Elle struct.
    // Non-keyword keys (int, string, bool) are silently ignored per plan spec.
    let mut pairs: Vec<(String, Value)> = Vec::new();
    struct_keyword_iter(val, |name, v| {
        pairs.push((name.to_string(), v));
    });

    for (field_name, field_val) in pairs {
        // nil means "field not set" — skip it.
        if field_val.is_nil() {
            continue;
        }

        let field_desc = msg_desc.get_field_by_name(&field_name).ok_or_else(|| {
            format!(
                "unknown field '{}' in message '{}'",
                field_name,
                msg_desc.full_name()
            )
        })?;

        if field_desc.is_map() {
            let pb_map = encode_map(field_val, &field_desc)?;
            msg.set_field(&field_desc, PbValue::Map(pb_map));
        } else if field_desc.is_list() {
            let pb_list = encode_repeated(field_val, &field_desc)?;
            msg.set_field(&field_desc, PbValue::List(pb_list));
        } else {
            let pb_val = elle_to_pb(field_val, &field_desc)
                .map_err(|e| format!("field '{}': {}", field_name, e))?;
            msg.set_field(&field_desc, pb_val);
        }
    }

    Ok(msg)
}

/// Encode an Elle array into a repeated protobuf list.
fn encode_repeated(val: Value, field: &FieldDescriptor) -> Result<Vec<PbValue>, String> {
    let items: Vec<Value> = if let Some(arr) = val.as_array() {
        arr.to_vec()
    } else if let Some(arr) = val.as_array_mut() {
        arr.borrow().to_vec()
    } else {
        return Err(format!(
            "field '{}': expected array for repeated field, got {}",
            field.name(),
            val.type_name()
        ));
    };

    let mut result = Vec::with_capacity(items.len());
    for item in items {
        if item.is_nil() {
            continue;
        }
        // For repeated message fields, wrap with the element kind descriptor.
        let pb_val = match field.kind() {
            Kind::Message(msg_desc) => {
                let dyn_msg = encode_message(item, &msg_desc)?;
                PbValue::Message(dyn_msg)
            }
            _ => elle_to_pb(item, field)?,
        };
        result.push(pb_val);
    }
    Ok(result)
}

/// Encode an Elle struct into a protobuf map.
fn encode_map(val: Value, field: &FieldDescriptor) -> Result<HashMap<MapKey, PbValue>, String> {
    let is_struct = val.as_struct().is_some() || val.as_struct_mut().is_some();
    if !is_struct {
        return Err(format!(
            "field '{}': expected struct for map field, got {}",
            field.name(),
            val.type_name()
        ));
    }

    let msg_desc = match field.kind() {
        Kind::Message(d) => d,
        _ => return Err(format!("field '{}': not a map field", field.name())),
    };

    let key_field = msg_desc
        .get_field_by_name("key")
        .ok_or_else(|| format!("field '{}': map entry has no 'key' field", field.name()))?;
    let value_field = msg_desc
        .get_field_by_name("value")
        .ok_or_else(|| format!("field '{}': map entry has no 'value' field", field.name()))?;

    // Collect all keys (keyword, int, bool, string) from the struct.
    let entries: Vec<(TableKey, Value)> = if let Some(s) = val.as_struct() {
        s.iter().map(|(k, v)| (k.clone(), *v)).collect()
    } else if let Some(s) = val.as_struct_mut() {
        s.borrow().iter().map(|(k, v)| (k.clone(), *v)).collect()
    } else {
        vec![]
    };

    let mut result = HashMap::new();
    for (key, map_val) in entries {
        if map_val.is_nil() {
            continue;
        }
        let map_key = elle_table_key_to_pb_map_key(key, &key_field)?;
        let pb_val = elle_to_pb(map_val, &value_field)
            .map_err(|e| format!("field '{}' map value: {}", field.name(), e))?;
        result.insert(map_key, pb_val);
    }
    Ok(result)
}

/// Convert an Elle `TableKey` to a protobuf `MapKey`.
fn elle_table_key_to_pb_map_key(
    key: TableKey,
    key_field: &FieldDescriptor,
) -> Result<MapKey, String> {
    match key_field.kind() {
        Kind::String => {
            // String map keys: Elle uses keyword keys (idiomatic)
            match key {
                TableKey::Keyword(name) => Ok(MapKey::String(name.to_string())),
                TableKey::String(s) => Ok(MapKey::String(s.to_string())),
                other => Err(format!(
                    "string map key must be a keyword or string key, got {:?}",
                    other
                )),
            }
        }
        Kind::Bool => match key {
            TableKey::Bool(b) => Ok(MapKey::Bool(b)),
            other => Err(format!("bool map key expected, got {:?}", other)),
        },
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => match key {
            TableKey::Int(n) => {
                if n < i32::MIN as i64 || n > i32::MAX as i64 {
                    Err(format!(
                        "map key {} out of int32 range [{}, {}]",
                        n,
                        i32::MIN,
                        i32::MAX
                    ))
                } else {
                    Ok(MapKey::I32(n as i32))
                }
            }
            other => Err(format!("int32 map key expected, got {:?}", other)),
        },
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => match key {
            TableKey::Int(n) => Ok(MapKey::I64(n)),
            other => Err(format!("int64 map key expected, got {:?}", other)),
        },
        Kind::Uint32 | Kind::Fixed32 => match key {
            TableKey::Int(n) => {
                if n < 0 || n > u32::MAX as i64 {
                    Err(format!(
                        "map key {} out of uint32 range [0, {}]",
                        n,
                        u32::MAX
                    ))
                } else {
                    Ok(MapKey::U32(n as u32))
                }
            }
            other => Err(format!("uint32 map key expected, got {:?}", other)),
        },
        Kind::Uint64 | Kind::Fixed64 => {
            // TODO(uint64): Elle has no u64 Value type; using string representation
            // for uint64/fixed64 map keys > i64::MAX. When u64 is added to Value,
            // change this to use int keys directly.
            match key {
                TableKey::Int(n) => {
                    // n is i64, but uint64 values > i64::MAX are encoded as strings.
                    Ok(MapKey::U64(n as u64))
                }
                TableKey::String(s) => {
                    // TODO(uint64): String representation for values > i64::MAX
                    let n: u64 = s
                        .parse()
                        .map_err(|_| format!("uint64 map key: cannot parse '{}' as u64", s))?;
                    Ok(MapKey::U64(n))
                }
                other => Err(format!(
                    "uint64 map key expected int or string (for values > 2^63-1), got {:?}",
                    other
                )),
            }
        }
        other => Err(format!("unsupported map key type {:?}", other)),
    }
}

// ---------------------------------------------------------------------------
// Protobuf → Elle (decode)
// ---------------------------------------------------------------------------

/// Convert a `prost_reflect::Value` to an Elle `Value` for the given field.
///
/// Returns `Err(String)` on conversion failure.
pub fn pb_to_elle(val: &PbValue, field: &FieldDescriptor) -> Result<Value, String> {
    match val {
        PbValue::Bool(b) => Ok(Value::bool(*b)),
        PbValue::I32(n) => Ok(Value::int(*n as i64)),
        PbValue::I64(n) => Ok(Value::int(*n)),
        PbValue::U32(n) => Ok(Value::int(*n as i64)),
        PbValue::U64(n) => {
            // Uint64: if it fits in i64, return as int; otherwise error.
            // Protobuf uint64 can hold values up to 2^64-1, but Elle int
            // is i64 signed (-2^63 to 2^63-1).
            //
            // For scalar uint64 fields: error if out of Elle range.
            // For map keys: handled separately in decode_map_key.
            const ELLE_INT_MAX: u64 = i64::MAX as u64;
            if *n > ELLE_INT_MAX {
                Err(format!(
                    "field '{}': uint64 value {} out of Elle i64 range",
                    field.name(),
                    n
                ))
            } else {
                Ok(Value::int(*n as i64))
            }
        }
        PbValue::F32(f) => Ok(Value::float(*f as f64)),
        PbValue::F64(f) => Ok(Value::float(*f)),
        PbValue::String(s) => Ok(Value::string(s.as_str())),
        PbValue::Bytes(b) => Ok(Value::bytes(b.as_ref().to_vec())),
        PbValue::EnumNumber(n) => {
            // Look up the enum value name.
            let enum_desc = match field.kind() {
                Kind::Enum(e) => e,
                _ => {
                    return Err(format!(
                        "field '{}': got EnumNumber but field is not enum",
                        field.name()
                    ));
                }
            };
            match enum_desc.get_value(*n) {
                Some(v) => Ok(Value::keyword(v.name())),
                // Unknown enum value: return as int (forward compatibility).
                None => Ok(Value::int(*n as i64)),
            }
        }
        PbValue::Message(dyn_msg) => {
            let struct_val = decode_message(dyn_msg)?;
            Ok(struct_val)
        }
        PbValue::List(items) => {
            // Determine element field for conversion.
            let element_vals: Result<Vec<Value>, String> =
                items.iter().map(|item| pb_to_elle(item, field)).collect();
            Ok(Value::array(element_vals?))
        }
        PbValue::Map(map) => {
            let struct_val = decode_map(map, field)?;
            Ok(struct_val)
        }
    }
}

/// Decode a `DynamicMessage` into an Elle immutable struct.
pub fn decode_message(msg: &DynamicMessage) -> Result<Value, String> {
    let mut fields: BTreeMap<TableKey, Value> = BTreeMap::new();

    for field in msg.descriptor().fields() {
        // Only include fields that are explicitly set.
        // For proto3 non-optional fields, unset = default = omitted.
        if !msg.has_field(&field) {
            continue;
        }

        let pb_val = msg.get_field(&field);
        let elle_val = if field.is_map() {
            decode_map_field(pb_val.as_ref(), &field)?
        } else if field.is_list() {
            decode_list_field(pb_val.as_ref(), &field)?
        } else {
            pb_to_elle(pb_val.as_ref(), &field)?
        };

        let key = TableKey::Keyword(field.name().into());
        fields.insert(key, elle_val);
    }

    Ok(Value::struct_from(fields))
}

/// Decode a map field (PbValue::Map) into an Elle struct.
fn decode_map_field(val: &PbValue, field: &FieldDescriptor) -> Result<Value, String> {
    match val {
        PbValue::Map(map) => decode_map(map, field),
        _ => Err(format!(
            "field '{}': expected Map, got {:?}",
            field.name(),
            val
        )),
    }
}

/// Decode a map into an Elle struct.
fn decode_map(map: &HashMap<MapKey, PbValue>, field: &FieldDescriptor) -> Result<Value, String> {
    let msg_desc = match field.kind() {
        Kind::Message(d) => d,
        _ => return Err(format!("field '{}': not a map field", field.name())),
    };

    let key_field = msg_desc
        .get_field_by_name("key")
        .ok_or_else(|| format!("field '{}': map entry has no 'key' field", field.name()))?;
    let value_field = msg_desc
        .get_field_by_name("value")
        .ok_or_else(|| format!("field '{}': map entry has no 'value' field", field.name()))?;

    let mut result: BTreeMap<TableKey, Value> = BTreeMap::new();
    for (k, v) in map {
        let table_key = pb_map_key_to_table_key(k, &key_field)?;
        let elle_val = pb_to_elle(v, &value_field)?;
        result.insert(table_key, elle_val);
    }

    Ok(Value::struct_from(result))
}

/// Convert a protobuf `MapKey` to an Elle `TableKey`.
fn pb_map_key_to_table_key(key: &MapKey, key_field: &FieldDescriptor) -> Result<TableKey, String> {
    match key {
        MapKey::String(s) => {
            // String map keys → keyword keys (idiomatic Elle)
            Ok(TableKey::Keyword(s.clone()))
        }
        MapKey::Bool(b) => Ok(TableKey::Bool(*b)),
        MapKey::I32(n) => Ok(TableKey::Int(*n as i64)),
        MapKey::I64(n) => Ok(TableKey::Int(*n)),
        MapKey::U32(n) => Ok(TableKey::Int(*n as i64)),
        MapKey::U64(n) => {
            // TODO(uint64): Elle has no u64 Value type; using string representation
            // for uint64/fixed64 map keys > i64::MAX. When u64 is added to Value,
            // change this to use int keys directly.
            const I64_MAX: u64 = i64::MAX as u64;
            let _ = key_field; // used for type context in encode path
            if *n <= I64_MAX {
                Ok(TableKey::Int(*n as i64))
            } else {
                // Represent as string for values > 2^63-1
                Ok(TableKey::String(n.to_string()))
            }
        }
    }
}

/// Decode a list field (PbValue::List) into an Elle immutable array.
fn decode_list_field(val: &PbValue, field: &FieldDescriptor) -> Result<Value, String> {
    match val {
        PbValue::List(items) => {
            let element_vals: Result<Vec<Value>, String> =
                items.iter().map(|item| pb_to_elle(item, field)).collect();
            Ok(Value::array(element_vals?))
        }
        _ => Err(format!(
            "field '{}': expected List, got {:?}",
            field.name(),
            val
        )),
    }
}

// ---------------------------------------------------------------------------
// Elle value extraction helpers
// ---------------------------------------------------------------------------

/// Extract an Elle integer value.
fn elle_int_val(val: Value) -> Result<i64, String> {
    val.as_int()
        .ok_or_else(|| format!("expected int, got {}", val.type_name()))
}

/// Extract and range-check an i32.
fn elle_to_i32(val: Value, field_name: &str) -> Result<i32, String> {
    let n = elle_int_val(val)?;
    if n < i32::MIN as i64 || n > i32::MAX as i64 {
        Err(format!(
            "value {} out of int32 range [{}, {}] for field '{}'",
            n,
            i32::MIN,
            i32::MAX,
            field_name
        ))
    } else {
        Ok(n as i32)
    }
}

/// Extract and range-check a u32.
fn elle_to_u32(val: Value, field_name: &str) -> Result<u32, String> {
    let n = elle_int_val(val)?;
    if n < 0 || n > u32::MAX as i64 {
        Err(format!(
            "value {} out of uint32 range [0, {}] for field '{}'",
            n,
            u32::MAX,
            field_name
        ))
    } else {
        Ok(n as u32)
    }
}

/// Extract and range-check a u64 (from Elle int or string for large values).
fn elle_to_u64(val: Value, field_name: &str) -> Result<u64, String> {
    if let Some(n) = val.as_int() {
        // Elle int is signed i64; non-negative values fit in u64.
        if n < 0 {
            Err(format!(
                "negative value {} cannot be encoded as uint64 for field '{}'",
                n, field_name
            ))
        } else {
            Ok(n as u64)
        }
    } else if let Some(s) = val.with_string(|s| s.to_string()) {
        s.parse::<u64>()
            .map_err(|_| format!("cannot parse '{}' as uint64 for field '{}'", s, field_name))
    } else {
        Err(format!(
            "expected int or string for uint64 field '{}', got {}",
            field_name,
            val.type_name()
        ))
    }
}

/// Extract a string from an Elle value (immutable or mutable string).
fn elle_to_string(val: Value) -> Result<String, String> {
    if let Some(s) = val.with_string(|s| s.to_string()) {
        return Ok(s);
    }
    if let Some(cell) = val.as_string_mut() {
        let bytes = cell.borrow();
        return String::from_utf8(bytes.to_vec())
            .map_err(|_| "string contains invalid UTF-8".to_string());
    }
    Err(format!("expected string, got {}", val.type_name()))
}

/// Extract bytes from an Elle value (immutable bytes, mutable @bytes, or string).
fn elle_to_bytes(val: Value) -> Result<Vec<u8>, String> {
    if let Some(b) = val.as_bytes() {
        return Ok(b.to_vec());
    }
    if let Some(b) = val.as_bytes_mut() {
        return Ok(b.borrow().to_vec());
    }
    if let Some(s) = val.with_string(|s| s.as_bytes().to_vec()) {
        return Ok(s);
    }
    Err(format!("expected bytes, got {}", val.type_name()))
}

// ---------------------------------------------------------------------------
// Encode/decode primitives (called from lib.rs)
// ---------------------------------------------------------------------------

/// Implement `protobuf/encode`: encode an Elle struct to protobuf bytes.
pub fn encode(args: &[Value]) -> (SignalBits, Value) {
    const PRIM: &str = "protobuf/encode";

    let pool = match crate::schema::get_pool(&args[0], PRIM) {
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

    let is_struct = args[2].as_struct().is_some() || args[2].as_struct_mut().is_some();
    if !is_struct {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected struct, got {}", PRIM, args[2].type_name()),
            ),
        );
    }

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

    match encode_message(args[2], &msg_desc) {
        Ok(dyn_msg) => {
            use prost::Message;
            let encoded = dyn_msg.encode_to_vec();
            (SIG_OK, Value::bytes(encoded))
        }
        Err(e) => (
            SIG_ERROR,
            error_val("protobuf-error", format!("{}: {}", PRIM, e)),
        ),
    }
}

/// Implement `protobuf/decode`: decode protobuf bytes to an Elle struct.
pub fn decode(args: &[Value]) -> (SignalBits, Value) {
    const PRIM: &str = "protobuf/decode";

    let pool = match crate::schema::get_pool(&args[0], PRIM) {
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

    let bytes = match crate::schema::extract_bytes(args[2], PRIM) {
        Ok(b) => b,
        Err(e) => return e,
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

    match DynamicMessage::decode(msg_desc, bytes.as_slice()) {
        Ok(dyn_msg) => match decode_message(&dyn_msg) {
            Ok(struct_val) => (SIG_OK, struct_val),
            Err(e) => (
                SIG_ERROR,
                error_val("protobuf-error", format!("{}: {}", PRIM, e)),
            ),
        },
        Err(e) => (
            SIG_ERROR,
            error_val("protobuf-error", format!("{}: {}", PRIM, e)),
        ),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::parse_proto_string;

    /// Test helper: look up a key in a sorted-struct slice.
    /// Introduced when `as_struct()` switched from BTreeMap to sorted
    /// slice; keeps the assertions readable.
    fn get_field<'a>(entries: &'a [(TableKey, Value)], key: &TableKey) -> Option<&'a Value> {
        entries
            .binary_search_by(|(k, _)| k.cmp(key))
            .ok()
            .map(|i| &entries[i].1)
    }

    const TEST_PROTO: &str = r#"
syntax = "proto3";
package test;

enum Status {
  UNKNOWN = 0;
  OK = 1;
  ERROR = 2;
}

message Person {
  string name = 1;
  int32 age = 2;
  repeated string tags = 3;
  Status status = 4;
  map<string, int32> scores = 5;
}

message Team {
  string team_name = 1;
  repeated Person members = 2;
}
"#;

    fn make_pool() -> prost_reflect::DescriptorPool {
        parse_proto_string(TEST_PROTO, "test.proto", &[]).expect("parse_proto_string failed")
    }

    #[test]
    fn test_roundtrip_person() {
        let pool = make_pool();
        let person_desc = pool.get_message_by_name("test.Person").unwrap();

        // Build Elle struct: {:name "Alice" :age 30 :tags ["dev" "lisp"]}
        let mut fields = BTreeMap::new();
        fields.insert(TableKey::Keyword("name".into()), Value::string("Alice"));
        fields.insert(TableKey::Keyword("age".into()), Value::int(30));
        fields.insert(
            TableKey::Keyword("tags".into()),
            Value::array(vec![Value::string("dev"), Value::string("lisp")]),
        );
        let elle_struct = Value::struct_from(fields);

        // Encode
        let dyn_msg = encode_message(elle_struct, &person_desc).expect("encode failed");
        let encoded = {
            use prost::Message;
            dyn_msg.encode_to_vec()
        };
        assert!(!encoded.is_empty(), "encoded bytes should not be empty");

        // Decode
        let decoded_msg =
            DynamicMessage::decode(person_desc, encoded.as_slice()).expect("decode failed");
        let decoded_struct = decode_message(&decoded_msg).expect("decode_message failed");

        // Verify :name
        let name_key = TableKey::Keyword("name".into());
        let name_val = get_field(decoded_struct.as_struct().unwrap(), &name_key).unwrap();
        assert!(
            name_val.with_string(|s| s == "Alice").unwrap_or(false),
            "name mismatch"
        );

        // Verify :age
        let age_key = TableKey::Keyword("age".into());
        let age_val = get_field(decoded_struct.as_struct().unwrap(), &age_key).unwrap();
        assert_eq!(age_val.as_int(), Some(30), "age mismatch");

        // Verify :tags
        let tags_key = TableKey::Keyword("tags".into());
        let tags_val = get_field(decoded_struct.as_struct().unwrap(), &tags_key).unwrap();
        let tags_arr = tags_val.as_array().expect("tags should be array");
        assert_eq!(tags_arr.len(), 2, "tags length mismatch");
        assert!(tags_arr[0].with_string(|s| s == "dev").unwrap_or(false));
        assert!(tags_arr[1].with_string(|s| s == "lisp").unwrap_or(false));
    }

    #[test]
    fn test_roundtrip_with_enum() {
        let pool = make_pool();
        let person_desc = pool.get_message_by_name("test.Person").unwrap();

        let mut fields = BTreeMap::new();
        fields.insert(TableKey::Keyword("name".into()), Value::string("Bob"));
        fields.insert(TableKey::Keyword("status".into()), Value::keyword("OK"));
        let elle_struct = Value::struct_from(fields);

        let dyn_msg = encode_message(elle_struct, &person_desc).expect("encode failed");
        let encoded = {
            use prost::Message;
            dyn_msg.encode_to_vec()
        };

        let decoded_msg =
            DynamicMessage::decode(person_desc, encoded.as_slice()).expect("decode failed");
        let decoded_struct = decode_message(&decoded_msg).expect("decode_message failed");

        let status_key = TableKey::Keyword("status".into());
        let status_val =
            get_field(decoded_struct.as_struct().unwrap(), &status_key).unwrap();
        // Enum keyword :OK
        assert_eq!(
            status_val.as_keyword_name().as_deref(),
            Some("OK"),
            "status should be :OK keyword"
        );
    }

    #[test]
    fn test_roundtrip_nested_team() {
        let pool = make_pool();
        let team_desc = pool.get_message_by_name("test.Team").unwrap();

        // Build: {:team-name "Alpha" :members [{:name "Alice" :age 30}]}
        let mut member_fields = BTreeMap::new();
        member_fields.insert(TableKey::Keyword("name".into()), Value::string("Alice"));
        member_fields.insert(TableKey::Keyword("age".into()), Value::int(30));
        let member = Value::struct_from(member_fields);

        let mut team_fields = BTreeMap::new();
        team_fields.insert(
            TableKey::Keyword("team_name".into()),
            Value::string("Alpha"),
        );
        team_fields.insert(
            TableKey::Keyword("members".into()),
            Value::array(vec![member]),
        );
        let elle_team = Value::struct_from(team_fields);

        let dyn_msg = encode_message(elle_team, &team_desc).expect("encode failed");
        let encoded = {
            use prost::Message;
            dyn_msg.encode_to_vec()
        };

        let decoded_msg =
            DynamicMessage::decode(team_desc, encoded.as_slice()).expect("decode failed");
        let decoded_struct = decode_message(&decoded_msg).expect("decode_message failed");

        let members_key = TableKey::Keyword("members".into());
        let members_val =
            get_field(decoded_struct.as_struct().unwrap(), &members_key).unwrap();
        let members_arr = members_val.as_array().expect("members should be array");
        assert_eq!(members_arr.len(), 1, "members length mismatch");

        let alice = members_arr[0];
        let name_key = TableKey::Keyword("name".into());
        let alice_name = get_field(alice.as_struct().unwrap(), &name_key).unwrap();
        assert!(
            alice_name.with_string(|s| s == "Alice").unwrap_or(false),
            "nested member name mismatch"
        );
    }

    #[test]
    fn test_roundtrip_map_field() {
        let pool = make_pool();
        let person_desc = pool.get_message_by_name("test.Person").unwrap();

        // {:name "Dave" :scores {:math 95 :science 88}}
        let mut scores_fields = BTreeMap::new();
        scores_fields.insert(TableKey::Keyword("math".into()), Value::int(95));
        scores_fields.insert(TableKey::Keyword("science".into()), Value::int(88));
        let scores = Value::struct_from(scores_fields);

        let mut fields = BTreeMap::new();
        fields.insert(TableKey::Keyword("name".into()), Value::string("Dave"));
        fields.insert(TableKey::Keyword("scores".into()), scores);
        let elle_struct = Value::struct_from(fields);

        let dyn_msg = encode_message(elle_struct, &person_desc).expect("encode failed");
        let encoded = {
            use prost::Message;
            dyn_msg.encode_to_vec()
        };

        let decoded_msg =
            DynamicMessage::decode(person_desc, encoded.as_slice()).expect("decode failed");
        let decoded_struct = decode_message(&decoded_msg).expect("decode_message failed");

        let scores_key = TableKey::Keyword("scores".into());
        let scores_val =
            get_field(decoded_struct.as_struct().unwrap(), &scores_key).unwrap();
        let scores_struct = scores_val.as_struct().expect("scores should be struct");

        let math_key = TableKey::Keyword("math".into());
        let math_val = get_field(scores_struct, &math_key).unwrap();
        assert_eq!(math_val.as_int(), Some(95), "math score mismatch");
    }

    #[test]
    fn test_int64_max() {
        let proto = r#"
syntax = "proto3";
message BigNum { int64 id = 1; }
"#;
        let pool = parse_proto_string(proto, "bignum.proto", &[]).unwrap();
        let desc = pool.get_message_by_name("BigNum").unwrap();

        let n = i64::MAX;
        let mut fields = BTreeMap::new();
        fields.insert(TableKey::Keyword("id".into()), Value::int(n));
        let elle_struct = Value::struct_from(fields);

        let dyn_msg = encode_message(elle_struct, &desc).expect("encode i64::MAX failed");
        let encoded = {
            use prost::Message;
            dyn_msg.encode_to_vec()
        };
        let decoded_msg = DynamicMessage::decode(desc, encoded.as_slice()).unwrap();
        let decoded = decode_message(&decoded_msg).unwrap();
        let id_key = TableKey::Keyword("id".into());
        let id_val = get_field(decoded.as_struct().unwrap(), &id_key).unwrap();
        assert_eq!(id_val.as_int(), Some(n), "int64 max roundtrip failed");
    }

    #[test]
    fn test_uint64_map_key_overflow() {
        // TODO(uint64): Elle has no u64 Value type; using string representation
        // for uint64/fixed64 map keys > i64::MAX. When u64 is added to Value,
        // change this to use int keys directly.
        let proto = r#"
syntax = "proto3";
message Counter { map<uint64, int32> counts = 1; }
"#;
        let pool = parse_proto_string(proto, "counter.proto", &[]).unwrap();
        let desc = pool.get_message_by_name("Counter").unwrap();

        // Encode: int key (fits in i64)
        let mut counts_fields = BTreeMap::new();
        counts_fields.insert(TableKey::Int(42), Value::int(100));
        let counts = Value::struct_from(counts_fields);

        let mut fields = BTreeMap::new();
        fields.insert(TableKey::Keyword("counts".into()), counts);
        let elle_struct = Value::struct_from(fields);

        let dyn_msg = encode_message(elle_struct, &desc).expect("encode uint64 key failed");
        let encoded = {
            use prost::Message;
            dyn_msg.encode_to_vec()
        };
        let decoded_msg = DynamicMessage::decode(desc, encoded.as_slice()).unwrap();
        let decoded = decode_message(&decoded_msg).unwrap();

        let counts_key = TableKey::Keyword("counts".into());
        let counts_val = get_field(decoded.as_struct().unwrap(), &counts_key).unwrap();
        let counts_struct = counts_val.as_struct().expect("counts should be struct");

        // uint64 key 42 fits in i64, so decoded as int key
        let key_42 = TableKey::Int(42);
        let val_42 = get_field(counts_struct, &key_42).unwrap();
        assert_eq!(
            val_42.as_int(),
            Some(100),
            "uint64 int key roundtrip failed"
        );
    }

    #[test]
    fn test_oneof_field() {
        let proto = r#"
syntax = "proto3";
message Event {
  oneof payload {
    string text = 1;
    int32 code = 2;
  }
}
"#;
        let pool = parse_proto_string(proto, "event.proto", &[]).unwrap();
        let desc = pool.get_message_by_name("Event").unwrap();

        // Encode with :text set
        let mut fields = BTreeMap::new();
        fields.insert(TableKey::Keyword("text".into()), Value::string("hello"));
        let elle_struct = Value::struct_from(fields);

        let dyn_msg = encode_message(elle_struct, &desc).expect("encode oneof failed");
        let encoded = {
            use prost::Message;
            dyn_msg.encode_to_vec()
        };
        let decoded_msg = DynamicMessage::decode(desc, encoded.as_slice()).unwrap();
        let decoded = decode_message(&decoded_msg).unwrap();

        // Only :text should be present (oneof)
        let decoded_map = decoded.as_struct().unwrap();
        let text_key = TableKey::Keyword("text".into());
        let code_key = TableKey::Keyword("code".into());
        assert!(
            get_field(decoded_map, &text_key).is_some(),
            "text should be present"
        );
        assert!(
            get_field(decoded_map, &code_key).is_none(),
            "code should be absent (oneof)"
        );

        let text_val = get_field(decoded_map, &text_key).unwrap();
        assert!(text_val.with_string(|s| s == "hello").unwrap_or(false));
    }

    #[test]
    fn test_repeated_nested_messages() {
        let proto = r#"
syntax = "proto3";
message Item { string label = 1; int32 value = 2; }
message Bag { repeated Item items = 1; }
"#;
        let pool = parse_proto_string(proto, "bag.proto", &[]).unwrap();
        let desc = pool.get_message_by_name("Bag").unwrap();

        let make_item = |label: &str, value: i64| {
            let mut f = BTreeMap::new();
            f.insert(TableKey::Keyword("label".into()), Value::string(label));
            f.insert(TableKey::Keyword("value".into()), Value::int(value));
            Value::struct_from(f)
        };

        let mut fields = BTreeMap::new();
        fields.insert(
            TableKey::Keyword("items".into()),
            Value::array(vec![make_item("a", 1), make_item("b", 2)]),
        );
        let elle_struct = Value::struct_from(fields);

        let dyn_msg = encode_message(elle_struct, &desc).expect("encode failed");
        let encoded = {
            use prost::Message;
            dyn_msg.encode_to_vec()
        };
        let decoded_msg = DynamicMessage::decode(desc, encoded.as_slice()).unwrap();
        let decoded = decode_message(&decoded_msg).unwrap();

        let items_key = TableKey::Keyword("items".into());
        let items = get_field(decoded.as_struct().unwrap(), &items_key).unwrap();
        let arr = items.as_array().expect("items should be array");
        assert_eq!(arr.len(), 2);

        let item0 = arr[0].as_struct().unwrap();
        let label_key = TableKey::Keyword("label".into());
        let val_key = TableKey::Keyword("value".into());
        assert!(get_field(item0, &label_key)
            .unwrap()
            .with_string(|s| s == "a")
            .unwrap_or(false));
        assert_eq!(get_field(item0, &val_key).unwrap().as_int(), Some(1));
    }
}
