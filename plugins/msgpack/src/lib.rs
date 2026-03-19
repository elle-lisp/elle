//! Elle MessagePack plugin — binary serialization for Elle values.

use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::{Arity, TableKey};
use elle::value::{error_val, Value};
use rmp::decode::{read_marker, RmpRead};
use rmp::Marker;
use std::collections::{BTreeMap, BTreeSet};

// ---------------------------------------------------------------------------
// Elle integer range (NaN-boxed 48-bit signed integers)
// ---------------------------------------------------------------------------

const ELLE_INT_MIN: i64 = -(1i64 << 47);
const ELLE_INT_MAX: i64 = (1i64 << 47) - 1;

fn checked_int(n: i64, prim_name: &str) -> Result<Value, String> {
    if !(ELLE_INT_MIN..=ELLE_INT_MAX).contains(&n) {
        return Err(format!(
            "msgpack/{}: integer {} out of Elle 48-bit range [{}, {}]",
            prim_name, n, ELLE_INT_MIN, ELLE_INT_MAX
        ));
    }
    Ok(Value::int(n))
}

// ---------------------------------------------------------------------------
// Ext type ID constants
// ---------------------------------------------------------------------------

const EXT_KEYWORD: i8 = 1;
const EXT_SET: i8 = 2;
const EXT_LIST: i8 = 3;
const EXT_SYMBOL: i8 = 4;

// ---------------------------------------------------------------------------
// Mode enum
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Interop,
    Tagged,
}

// ---------------------------------------------------------------------------
// Encode helpers
// ---------------------------------------------------------------------------

fn encode_value(buf: &mut Vec<u8>, val: &Value, mode: Mode) -> Result<(), String> {
    if val.is_nil() {
        rmp::encode::write_nil(buf).unwrap();
    } else if val.is_empty_list() {
        match mode {
            Mode::Interop => {
                rmp::encode::write_array_len(buf, 0).unwrap();
            }
            Mode::Tagged => {
                let mut payload = Vec::new();
                rmp::encode::write_array_len(&mut payload, 0).unwrap();
                rmp::encode::write_ext_meta(buf, payload.len() as u32, EXT_LIST).unwrap();
                buf.extend_from_slice(&payload);
            }
        }
    } else if let Some(b) = val.as_bool() {
        rmp::encode::write_bool(buf, b).unwrap();
    } else if let Some(n) = val.as_int() {
        rmp::encode::write_sint(buf, n).unwrap();
    } else if let Some(f) = val.as_float() {
        rmp::encode::write_f64(buf, f).unwrap();
    } else if let Some(name) = val.as_keyword_name() {
        match mode {
            Mode::Interop => {
                rmp::encode::write_str(buf, &name).unwrap();
            }
            Mode::Tagged => {
                let mut payload = Vec::new();
                rmp::encode::write_str(&mut payload, &name).unwrap();
                rmp::encode::write_ext_meta(buf, payload.len() as u32, EXT_KEYWORD).unwrap();
                buf.extend_from_slice(&payload);
            }
        }
    } else if let Some(s) = val.with_string(|s| s.to_string()) {
        rmp::encode::write_str(buf, &s).unwrap();
    } else if let Some(cell) = val.as_string_mut() {
        let bytes = cell.borrow();
        let s = std::str::from_utf8(&bytes).map_err(|_| {
            format!(
                "msgpack/{}: @string contains invalid UTF-8",
                if mode == Mode::Tagged {
                    "encode-tagged"
                } else {
                    "encode"
                }
            )
        })?;
        rmp::encode::write_str(buf, s).unwrap();
    } else if let Some(data) = val.as_bytes() {
        rmp::encode::write_bin(buf, data).unwrap();
    } else if let Some(cell) = val.as_bytes_mut() {
        let data = cell.borrow();
        rmp::encode::write_bin(buf, &data).unwrap();
    } else if let Some(set) = val.as_set() {
        encode_set_elements(buf, set.iter(), set.len(), mode)?;
    } else if let Some(cell) = val.as_set_mut() {
        let set = cell.borrow();
        encode_set_elements(buf, set.iter(), set.len(), mode)?;
    } else if let Some(elems) = val.as_array() {
        rmp::encode::write_array_len(buf, elems.len() as u32).unwrap();
        for elem in elems {
            encode_value(buf, elem, mode)?;
        }
    } else if let Some(cell) = val.as_array_mut() {
        let arr = cell.borrow();
        rmp::encode::write_array_len(buf, arr.len() as u32).unwrap();
        for elem in arr.iter() {
            encode_value(buf, elem, mode)?;
        }
    } else if let Some(fields) = val.as_struct() {
        rmp::encode::write_map_len(buf, fields.len() as u32).unwrap();
        for (k, v) in fields.iter() {
            encode_map_key(buf, k, mode)?;
            encode_value(buf, v, mode)?;
        }
    } else if let Some(cell) = val.as_struct_mut() {
        let fields = cell.borrow();
        rmp::encode::write_map_len(buf, fields.len() as u32).unwrap();
        for (k, v) in fields.iter() {
            encode_map_key(buf, k, mode)?;
            encode_value(buf, v, mode)?;
        }
    } else if val.as_cons().is_some() {
        let prim_name = if mode == Mode::Tagged {
            "encode-tagged"
        } else {
            "encode"
        };
        let elements = val
            .list_to_vec()
            .map_err(|_| format!("msgpack/{}: improper list", prim_name))?;
        match mode {
            Mode::Interop => {
                rmp::encode::write_array_len(buf, elements.len() as u32).unwrap();
                for elem in &elements {
                    encode_value(buf, elem, mode)?;
                }
            }
            Mode::Tagged => {
                let mut payload = Vec::new();
                rmp::encode::write_array_len(&mut payload, elements.len() as u32).unwrap();
                for elem in &elements {
                    encode_value(&mut payload, elem, mode)?;
                }
                rmp::encode::write_ext_meta(buf, payload.len() as u32, EXT_LIST).unwrap();
                buf.extend_from_slice(&payload);
            }
        }
    } else {
        let prim_name = if mode == Mode::Tagged {
            "encode-tagged"
        } else {
            "encode"
        };
        if val.as_symbol().is_some() {
            return Err(format!(
                "msgpack/{}: cannot encode symbol (name resolution unavailable in plugins)",
                prim_name
            ));
        }
        return Err(format!(
            "msgpack/{}: cannot encode {}",
            prim_name,
            val.type_name()
        ));
    }
    Ok(())
}

fn encode_set_elements<'a>(
    buf: &mut Vec<u8>,
    iter: impl Iterator<Item = &'a Value>,
    len: usize,
    mode: Mode,
) -> Result<(), String> {
    match mode {
        Mode::Interop => {
            rmp::encode::write_array_len(buf, len as u32).unwrap();
            for elem in iter {
                encode_value(buf, elem, mode)?;
            }
        }
        Mode::Tagged => {
            let mut payload = Vec::new();
            rmp::encode::write_array_len(&mut payload, len as u32).unwrap();
            for elem in iter {
                encode_value(&mut payload, elem, mode)?;
            }
            rmp::encode::write_ext_meta(buf, payload.len() as u32, EXT_SET).unwrap();
            buf.extend_from_slice(&payload);
        }
    }
    Ok(())
}

fn encode_map_key(buf: &mut Vec<u8>, key: &TableKey, mode: Mode) -> Result<(), String> {
    match key {
        TableKey::Keyword(name) => match mode {
            Mode::Interop => {
                rmp::encode::write_str(buf, name).unwrap();
            }
            Mode::Tagged => {
                let mut payload = Vec::new();
                rmp::encode::write_str(&mut payload, name).unwrap();
                rmp::encode::write_ext_meta(buf, payload.len() as u32, EXT_KEYWORD).unwrap();
                buf.extend_from_slice(&payload);
            }
        },
        TableKey::String(s) => {
            rmp::encode::write_str(buf, s).unwrap();
        }
        TableKey::Int(n) => {
            rmp::encode::write_sint(buf, *n).unwrap();
        }
        TableKey::Bool(b) => {
            rmp::encode::write_bool(buf, *b).unwrap();
        }
        TableKey::Nil => {
            rmp::encode::write_nil(buf).unwrap();
        }
        TableKey::Symbol(_) => {
            let prim_name = if mode == Mode::Tagged {
                "encode-tagged"
            } else {
                "encode"
            };
            return Err(format!(
                "msgpack/{}: cannot encode symbol as map key",
                prim_name
            ));
        }
        TableKey::Identity(_) => {
            let prim_name = if mode == Mode::Tagged {
                "encode-tagged"
            } else {
                "encode"
            };
            return Err(format!(
                "msgpack/{}: cannot encode identity key as map key",
                prim_name
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Decode helpers
// ---------------------------------------------------------------------------

fn decode_value(rd: &mut &[u8], mode: Mode) -> Result<Value, String> {
    let prim_name = if mode == Mode::Tagged {
        "decode-tagged"
    } else {
        "decode"
    };
    let marker = read_marker(rd).map_err(|e| format!("msgpack/{}: {}", prim_name, e.0))?;
    decode_value_from_marker(rd, marker, mode, prim_name)
}

fn decode_value_from_marker(
    rd: &mut &[u8],
    marker: Marker,
    mode: Mode,
    prim_name: &str,
) -> Result<Value, String> {
    match marker {
        Marker::Null => Ok(Value::NIL),
        Marker::True => Ok(Value::TRUE),
        Marker::False => Ok(Value::FALSE),
        Marker::FixPos(n) => Ok(Value::int(n as i64)),
        Marker::FixNeg(n) => Ok(Value::int(n as i64)),
        Marker::U8 => {
            let n = rd.read_data_u8().map_err(|e| fmt_vread_err(prim_name, e))?;
            Ok(Value::int(n as i64))
        }
        Marker::U16 => {
            let n = rd
                .read_data_u16()
                .map_err(|e| fmt_vread_err(prim_name, e))?;
            Ok(Value::int(n as i64))
        }
        Marker::U32 => {
            let n = rd
                .read_data_u32()
                .map_err(|e| fmt_vread_err(prim_name, e))?;
            Ok(Value::int(n as i64))
        }
        Marker::U64 => {
            let n = rd
                .read_data_u64()
                .map_err(|e| fmt_vread_err(prim_name, e))?;
            if n > ELLE_INT_MAX as u64 {
                return Err(format!(
                    "msgpack/{}: uint64 value {} out of Elle 48-bit range",
                    prim_name, n
                ));
            }
            Ok(Value::int(n as i64))
        }
        Marker::I8 => {
            let n = rd.read_data_i8().map_err(|e| fmt_vread_err(prim_name, e))?;
            Ok(Value::int(n as i64))
        }
        Marker::I16 => {
            let n = rd
                .read_data_i16()
                .map_err(|e| fmt_vread_err(prim_name, e))?;
            Ok(Value::int(n as i64))
        }
        Marker::I32 => {
            let n = rd
                .read_data_i32()
                .map_err(|e| fmt_vread_err(prim_name, e))?;
            Ok(Value::int(n as i64))
        }
        Marker::I64 => {
            let n = rd
                .read_data_i64()
                .map_err(|e| fmt_vread_err(prim_name, e))?;
            checked_int(n, prim_name)
        }
        Marker::F32 => {
            let f = rd
                .read_data_f32()
                .map_err(|e| fmt_vread_err(prim_name, e))?;
            Ok(Value::float(f as f64))
        }
        Marker::F64 => {
            let f = rd
                .read_data_f64()
                .map_err(|e| fmt_vread_err(prim_name, e))?;
            Ok(Value::float(f))
        }
        Marker::FixStr(len) => decode_string(rd, len as u32, prim_name),
        Marker::Str8 => {
            let len = rd.read_data_u8().map_err(|e| fmt_vread_err(prim_name, e))? as u32;
            decode_string(rd, len, prim_name)
        }
        Marker::Str16 => {
            let len = rd
                .read_data_u16()
                .map_err(|e| fmt_vread_err(prim_name, e))? as u32;
            decode_string(rd, len, prim_name)
        }
        Marker::Str32 => {
            let len = rd
                .read_data_u32()
                .map_err(|e| fmt_vread_err(prim_name, e))?;
            decode_string(rd, len, prim_name)
        }
        Marker::Bin8 => {
            let len = rd.read_data_u8().map_err(|e| fmt_vread_err(prim_name, e))? as u32;
            decode_bytes(rd, len, prim_name)
        }
        Marker::Bin16 => {
            let len = rd
                .read_data_u16()
                .map_err(|e| fmt_vread_err(prim_name, e))? as u32;
            decode_bytes(rd, len, prim_name)
        }
        Marker::Bin32 => {
            let len = rd
                .read_data_u32()
                .map_err(|e| fmt_vread_err(prim_name, e))?;
            decode_bytes(rd, len, prim_name)
        }
        Marker::FixArray(len) => decode_array(rd, len as u32, mode, prim_name),
        Marker::Array16 => {
            let len = rd
                .read_data_u16()
                .map_err(|e| fmt_vread_err(prim_name, e))? as u32;
            decode_array(rd, len, mode, prim_name)
        }
        Marker::Array32 => {
            let len = rd
                .read_data_u32()
                .map_err(|e| fmt_vread_err(prim_name, e))?;
            decode_array(rd, len, mode, prim_name)
        }
        Marker::FixMap(len) => decode_map(rd, len as u32, mode, prim_name),
        Marker::Map16 => {
            let len = rd
                .read_data_u16()
                .map_err(|e| fmt_vread_err(prim_name, e))? as u32;
            decode_map(rd, len, mode, prim_name)
        }
        Marker::Map32 => {
            let len = rd
                .read_data_u32()
                .map_err(|e| fmt_vread_err(prim_name, e))?;
            decode_map(rd, len, mode, prim_name)
        }
        m @ (Marker::FixExt1
        | Marker::FixExt2
        | Marker::FixExt4
        | Marker::FixExt8
        | Marker::FixExt16
        | Marker::Ext8
        | Marker::Ext16
        | Marker::Ext32) => decode_ext(rd, m, mode, prim_name),
        Marker::Reserved => Err(format!("msgpack/{}: reserved marker (0xc1)", prim_name)),
    }
}

fn decode_string(rd: &mut &[u8], len: u32, prim_name: &str) -> Result<Value, String> {
    let mut data = vec![0u8; len as usize];
    rd.read_exact_buf(&mut data)
        .map_err(|e| format!("msgpack/{}: {}", prim_name, e))?;
    let s = std::str::from_utf8(&data)
        .map_err(|_| format!("msgpack/{}: invalid UTF-8 in string", prim_name))?;
    Ok(Value::string(s))
}

fn decode_bytes(rd: &mut &[u8], len: u32, prim_name: &str) -> Result<Value, String> {
    let mut data = vec![0u8; len as usize];
    rd.read_exact_buf(&mut data)
        .map_err(|e| format!("msgpack/{}: {}", prim_name, e))?;
    Ok(Value::bytes(data))
}

fn decode_array(rd: &mut &[u8], len: u32, mode: Mode, _prim_name: &str) -> Result<Value, String> {
    let mut elements = Vec::with_capacity(len as usize);
    for _ in 0..len {
        elements.push(decode_value(rd, mode)?);
    }
    Ok(Value::array(elements))
}

fn decode_map(rd: &mut &[u8], len: u32, mode: Mode, prim_name: &str) -> Result<Value, String> {
    let mut fields = BTreeMap::new();
    for _ in 0..len {
        let key = decode_map_key(rd, mode, prim_name)?;
        let val = decode_value(rd, mode)?;
        fields.insert(key, val);
    }
    Ok(Value::struct_from(fields))
}

fn decode_map_key(rd: &mut &[u8], mode: Mode, prim_name: &str) -> Result<TableKey, String> {
    let marker = read_marker(rd).map_err(|e| format!("msgpack/{}: {}", prim_name, e.0))?;

    // In tagged mode, intercept ext markers: ext(1) means keyword key
    if mode == Mode::Tagged {
        if let m @ (Marker::FixExt1
        | Marker::FixExt2
        | Marker::FixExt4
        | Marker::FixExt8
        | Marker::FixExt16
        | Marker::Ext8
        | Marker::Ext16
        | Marker::Ext32) = marker
        {
            let (typeid, size) = read_ext_type_and_size(rd, m, prim_name)?;
            if typeid == EXT_KEYWORD {
                let mut payload = vec![0u8; size as usize];
                rd.read_exact_buf(&mut payload)
                    .map_err(|e| format!("msgpack/{}: {}", prim_name, e))?;
                let name_val = decode_value(&mut payload.as_slice(), mode)?;
                if let Some(s) = name_val.with_string(|s| s.to_string()) {
                    return Ok(TableKey::Keyword(s));
                } else {
                    return Err(format!(
                        "msgpack/{}: ext(1) keyword payload must be a string",
                        prim_name
                    ));
                }
            } else {
                return Err(format!(
                    "msgpack/{}: unsupported map key type: ext({})",
                    prim_name, typeid
                ));
            }
        }
    }

    // For non-ext markers (or interop mode), decode the value and convert to key
    let val = decode_value_from_marker(rd, marker, mode, prim_name)?;
    value_to_map_key(val, prim_name)
}

fn decode_ext(
    rd: &mut &[u8],
    marker: Marker,
    mode: Mode,
    prim_name: &str,
) -> Result<Value, String> {
    if mode == Mode::Interop {
        return Err(format!(
            "msgpack/{}: ext types not supported in interop mode",
            prim_name
        ));
    }

    let (typeid, size) = read_ext_type_and_size(rd, marker, prim_name)?;

    let mut payload = vec![0u8; size as usize];
    rd.read_exact_buf(&mut payload)
        .map_err(|e| format!("msgpack/{}: {}", prim_name, e))?;

    match typeid {
        EXT_KEYWORD => {
            let name_val = decode_value(&mut payload.as_slice(), mode)?;
            match name_val.with_string(|s| s.to_string()) {
                Some(name) => Ok(Value::keyword(&name)),
                None => Err(format!(
                    "msgpack/{}: ext(1) keyword payload must be a string, got {}",
                    prim_name,
                    name_val.type_name()
                )),
            }
        }
        EXT_SET => {
            let arr_val = decode_value(&mut payload.as_slice(), mode)?;
            match arr_val.as_array() {
                Some(elems) => {
                    let btree: BTreeSet<Value> = elems.iter().copied().collect();
                    Ok(Value::set(btree))
                }
                None => Err(format!(
                    "msgpack/{}: ext(2) set payload must be an array",
                    prim_name
                )),
            }
        }
        EXT_LIST => {
            let arr_val = decode_value(&mut payload.as_slice(), mode)?;
            match arr_val.as_array() {
                Some(elems) => Ok(elle::list(elems.iter().copied())),
                None => Err(format!(
                    "msgpack/{}: ext(3) list payload must be an array",
                    prim_name
                )),
            }
        }
        EXT_SYMBOL => Err(format!(
            "msgpack/{}: cannot decode symbol (name resolution unavailable in plugins)",
            prim_name
        )),
        other => Err(format!("msgpack/{}: unknown ext type {}", prim_name, other)),
    }
}

fn fmt_vread_err(prim_name: &str, e: rmp::decode::ValueReadError<std::io::Error>) -> String {
    format!("msgpack/{}: {:?}", prim_name, e)
}

fn read_ext_type_and_size(
    rd: &mut &[u8],
    marker: Marker,
    prim_name: &str,
) -> Result<(i8, u32), String> {
    let size = match marker {
        Marker::FixExt1 => 1u32,
        Marker::FixExt2 => 2,
        Marker::FixExt4 => 4,
        Marker::FixExt8 => 8,
        Marker::FixExt16 => 16,
        Marker::Ext8 => rd.read_data_u8().map_err(|e| fmt_vread_err(prim_name, e))? as u32,
        Marker::Ext16 => rd
            .read_data_u16()
            .map_err(|e| fmt_vread_err(prim_name, e))? as u32,
        Marker::Ext32 => rd
            .read_data_u32()
            .map_err(|e| fmt_vread_err(prim_name, e))?,
        _ => unreachable!("read_ext_type_and_size called with non-ext marker"),
    };
    let typeid = rd.read_data_i8().map_err(|e| fmt_vread_err(prim_name, e))?;
    Ok((typeid, size))
}

fn value_to_map_key(val: Value, prim_name: &str) -> Result<TableKey, String> {
    if val.is_nil() {
        Ok(TableKey::Nil)
    } else if let Some(b) = val.as_bool() {
        Ok(TableKey::Bool(b))
    } else if let Some(n) = val.as_int() {
        Ok(TableKey::Int(n))
    } else if let Some(s) = val.with_string(|s| s.to_string()) {
        Ok(TableKey::String(s))
    } else {
        Err(format!(
            "msgpack/{}: unsupported map key type: {}",
            prim_name,
            val.type_name()
        ))
    }
}

// ---------------------------------------------------------------------------
// Validate (structural validity check, no Elle value construction)
// ---------------------------------------------------------------------------

fn validate(rd: &mut &[u8]) -> bool {
    validate_value(rd)
}

fn validate_value(rd: &mut &[u8]) -> bool {
    let marker = match read_marker(rd) {
        Ok(m) => m,
        Err(_) => return false,
    };
    validate_from_marker(rd, marker)
}

fn validate_from_marker(rd: &mut &[u8], marker: Marker) -> bool {
    match marker {
        Marker::Null | Marker::True | Marker::False => true,
        Marker::FixPos(_) | Marker::FixNeg(_) => true,
        Marker::U8 | Marker::I8 => skip_bytes(rd, 1),
        Marker::U16 | Marker::I16 => skip_bytes(rd, 2),
        Marker::U32 | Marker::I32 | Marker::F32 => skip_bytes(rd, 4),
        Marker::U64 | Marker::I64 | Marker::F64 => skip_bytes(rd, 8),
        Marker::FixStr(len) => skip_bytes(rd, len as usize),
        Marker::Str8 => {
            let len = match read_u8_raw(rd) {
                Some(n) => n as usize,
                None => return false,
            };
            skip_bytes(rd, len)
        }
        Marker::Str16 => {
            let len = match read_u16_be(rd) {
                Some(n) => n as usize,
                None => return false,
            };
            skip_bytes(rd, len)
        }
        Marker::Str32 => {
            let len = match read_u32_be(rd) {
                Some(n) => n as usize,
                None => return false,
            };
            skip_bytes(rd, len)
        }
        Marker::Bin8 => {
            let len = match read_u8_raw(rd) {
                Some(n) => n as usize,
                None => return false,
            };
            skip_bytes(rd, len)
        }
        Marker::Bin16 => {
            let len = match read_u16_be(rd) {
                Some(n) => n as usize,
                None => return false,
            };
            skip_bytes(rd, len)
        }
        Marker::Bin32 => {
            let len = match read_u32_be(rd) {
                Some(n) => n as usize,
                None => return false,
            };
            skip_bytes(rd, len)
        }
        Marker::FixArray(len) => {
            for _ in 0..len {
                if !validate_value(rd) {
                    return false;
                }
            }
            true
        }
        Marker::Array16 => {
            let len = match read_u16_be(rd) {
                Some(n) => n,
                None => return false,
            };
            for _ in 0..len {
                if !validate_value(rd) {
                    return false;
                }
            }
            true
        }
        Marker::Array32 => {
            let len = match read_u32_be(rd) {
                Some(n) => n,
                None => return false,
            };
            for _ in 0..len {
                if !validate_value(rd) {
                    return false;
                }
            }
            true
        }
        Marker::FixMap(len) => {
            for _ in 0..len {
                if !validate_value(rd) {
                    return false;
                }
                if !validate_value(rd) {
                    return false;
                }
            }
            true
        }
        Marker::Map16 => {
            let len = match read_u16_be(rd) {
                Some(n) => n,
                None => return false,
            };
            for _ in 0..len {
                if !validate_value(rd) {
                    return false;
                }
                if !validate_value(rd) {
                    return false;
                }
            }
            true
        }
        Marker::Map32 => {
            let len = match read_u32_be(rd) {
                Some(n) => n,
                None => return false,
            };
            for _ in 0..len {
                if !validate_value(rd) {
                    return false;
                }
                if !validate_value(rd) {
                    return false;
                }
            }
            true
        }
        // Ext types: valid msgpack; skip the payload without validating contents
        Marker::FixExt1 => skip_bytes(rd, 1 + 1),
        Marker::FixExt2 => skip_bytes(rd, 1 + 2),
        Marker::FixExt4 => skip_bytes(rd, 1 + 4),
        Marker::FixExt8 => skip_bytes(rd, 1 + 8),
        Marker::FixExt16 => skip_bytes(rd, 1 + 16),
        Marker::Ext8 => {
            let len = match read_u8_raw(rd) {
                Some(n) => n as usize,
                None => return false,
            };
            skip_bytes(rd, 1 + len)
        }
        Marker::Ext16 => {
            let len = match read_u16_be(rd) {
                Some(n) => n as usize,
                None => return false,
            };
            skip_bytes(rd, 1 + len)
        }
        Marker::Ext32 => {
            let len = match read_u32_be(rd) {
                Some(n) => n as usize,
                None => return false,
            };
            skip_bytes(rd, 1 + len)
        }
        Marker::Reserved => false,
    }
}

fn skip_bytes(rd: &mut &[u8], n: usize) -> bool {
    if rd.len() < n {
        return false;
    }
    *rd = &rd[n..];
    true
}

fn read_u8_raw(rd: &mut &[u8]) -> Option<u8> {
    if rd.is_empty() {
        return None;
    }
    let b = rd[0];
    *rd = &rd[1..];
    Some(b)
}

fn read_u16_be(rd: &mut &[u8]) -> Option<u16> {
    if rd.len() < 2 {
        return None;
    }
    let n = u16::from_be_bytes([rd[0], rd[1]]);
    *rd = &rd[2..];
    Some(n)
}

fn read_u32_be(rd: &mut &[u8]) -> Option<u32> {
    if rd.len() < 4 {
        return None;
    }
    let n = u32::from_be_bytes([rd[0], rd[1], rd[2], rd[3]]);
    *rd = &rd[4..];
    Some(n)
}

// ---------------------------------------------------------------------------
// Primitive wrappers
// ---------------------------------------------------------------------------

fn prim_msgpack_encode(args: &[Value]) -> (SignalBits, Value) {
    let mut buf = Vec::new();
    match encode_value(&mut buf, &args[0], Mode::Interop) {
        Ok(()) => (SIG_OK, Value::bytes(buf)),
        Err(msg) => (SIG_ERROR, error_val("msgpack-error", msg)),
    }
}

fn prim_msgpack_decode(args: &[Value]) -> (SignalBits, Value) {
    let raw: Vec<u8>;
    let data: &[u8] = if let Some(b) = args[0].as_bytes() {
        b
    } else if let Some(cell) = args[0].as_bytes_mut() {
        raw = cell.borrow().clone();
        &raw
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "msgpack/decode: expected bytes, got {}",
                    args[0].type_name()
                ),
            ),
        );
    };

    let mut rd = data;
    match decode_value(&mut rd, Mode::Interop) {
        Ok(val) => {
            if !rd.is_empty() {
                (
                    SIG_ERROR,
                    error_val(
                        "msgpack-error",
                        format!("msgpack/decode: {} trailing bytes after value", rd.len()),
                    ),
                )
            } else {
                (SIG_OK, val)
            }
        }
        Err(msg) => (SIG_ERROR, error_val("msgpack-error", msg)),
    }
}

fn prim_msgpack_valid(args: &[Value]) -> (SignalBits, Value) {
    let raw: Vec<u8>;
    let data: &[u8] = if let Some(b) = args[0].as_bytes() {
        b
    } else if let Some(cell) = args[0].as_bytes_mut() {
        raw = cell.borrow().clone();
        &raw
    } else {
        return (SIG_OK, Value::FALSE);
    };

    if data.is_empty() {
        return (SIG_OK, Value::FALSE);
    }

    let mut rd = data;
    let ok = validate(&mut rd);
    let result = ok && rd.is_empty();
    (SIG_OK, Value::bool(result))
}

fn prim_msgpack_encode_tagged(args: &[Value]) -> (SignalBits, Value) {
    let mut buf = Vec::new();
    match encode_value(&mut buf, &args[0], Mode::Tagged) {
        Ok(()) => (SIG_OK, Value::bytes(buf)),
        Err(msg) => (SIG_ERROR, error_val("msgpack-error", msg)),
    }
}

fn prim_msgpack_decode_tagged(args: &[Value]) -> (SignalBits, Value) {
    let raw: Vec<u8>;
    let data: &[u8] = if let Some(b) = args[0].as_bytes() {
        b
    } else if let Some(cell) = args[0].as_bytes_mut() {
        raw = cell.borrow().clone();
        &raw
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "msgpack/decode-tagged: expected bytes, got {}",
                    args[0].type_name()
                ),
            ),
        );
    };

    let mut rd = data;
    match decode_value(&mut rd, Mode::Tagged) {
        Ok(val) => {
            if !rd.is_empty() {
                (
                    SIG_ERROR,
                    error_val(
                        "msgpack-error",
                        format!(
                            "msgpack/decode-tagged: {} trailing bytes after value",
                            rd.len()
                        ),
                    ),
                )
            } else {
                (SIG_OK, val)
            }
        }
        Err(msg) => (SIG_ERROR, error_val("msgpack-error", msg)),
    }
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "msgpack/encode",
        func: prim_msgpack_encode,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Encode an Elle value to msgpack bytes (interop mode)",
        params: &["value"],
        category: "msgpack",
        example: r#"(msgpack/encode {:x 1 :y "hello"})"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "msgpack/decode",
        func: prim_msgpack_decode,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Decode msgpack bytes to an Elle value (interop mode)",
        params: &["bytes"],
        category: "msgpack",
        example: r#"(msgpack/decode (msgpack/encode 42))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "msgpack/valid?",
        func: prim_msgpack_valid,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Check if bytes are structurally valid msgpack",
        params: &["bytes"],
        category: "msgpack",
        example: r#"(msgpack/valid? (msgpack/encode 42))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "msgpack/encode-tagged",
        func: prim_msgpack_encode_tagged,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Encode an Elle value to msgpack bytes with ext types for keywords, sets, lists",
        params: &["value"],
        category: "msgpack",
        example: r#"(msgpack/encode-tagged {:x 1 :y (list 2 3)})"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "msgpack/decode-tagged",
        func: prim_msgpack_decode_tagged,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Decode msgpack bytes with Elle ext types",
        params: &["bytes"],
        category: "msgpack",
        example: r#"(msgpack/decode-tagged (msgpack/encode-tagged :hello))"#,
        aliases: &[],
    },
];

// ---------------------------------------------------------------------------
// Plugin entry point
// ---------------------------------------------------------------------------

/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`. The caller must pass a valid
/// `PluginContext` reference. Only safe when called from `load_plugin`.
#[no_mangle]
pub unsafe extern "C" fn elle_plugin_init(ctx: &mut PluginContext) -> Value {
    // MUST be called first, before any keyword operation.
    ctx.init_keywords();

    let mut fields = BTreeMap::new();
    for def in PRIMITIVES {
        ctx.register(def);
        let short_name = def.name.strip_prefix("msgpack/").unwrap_or(def.name);
        fields.insert(
            TableKey::Keyword(short_name.into()),
            Value::native_fn(def.func),
        );
    }
    Value::struct_from(fields)
}
