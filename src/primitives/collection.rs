//! Collection protocol: centralized dispatch for all container types.
//!
//! Every container in Elle (list, (), array, @array, string, @string,
//! bytes, @bytes, set, @set, struct, @struct) implements these operations.
//! Each function dispatches once; primitives delegate here instead of
//! repeating the 12-way type match.
use crate::value::{error_val, sorted_struct_contains, TableKey, Value};
use unicode_segmentation::UnicodeSegmentation;

use super::sets::freeze_value;

/// Is the collection empty?
pub fn coll_empty(val: &Value) -> Result<bool, Value> {
    if val.is_nil() {
        return Err(error_val("type-error", "expected collection type, got nil"));
    }
    if val.is_empty_list() {
        return Ok(true);
    }
    if val.is_pair() {
        return Ok(false);
    }
    if let Some(elems) = val.as_array() {
        return Ok(elems.is_empty());
    }
    if let Some(arr) = val.as_array_mut() {
        return Ok(arr.borrow().is_empty());
    }
    if let Some(r) = val.with_string(|s| s.is_empty()) {
        return Ok(r);
    }
    if let Some(buf_ref) = val.as_string_mut() {
        return Ok(buf_ref.borrow().is_empty());
    }
    if let Some(b) = val.as_bytes() {
        return Ok(b.is_empty());
    }
    if let Some(blob_ref) = val.as_bytes_mut() {
        return Ok(blob_ref.borrow().is_empty());
    }
    if let Some(s) = val.as_set() {
        return Ok(s.is_empty());
    }
    if let Some(s) = val.as_set_mut() {
        return Ok(s.borrow().is_empty());
    }
    if let Some(s) = val.as_struct() {
        return Ok(s.is_empty());
    }
    if let Some(t) = val.as_struct_mut() {
        return Ok(t.borrow().is_empty());
    }
    if let Some(syntax) = val.as_syntax() {
        use crate::syntax::SyntaxKind;
        if let SyntaxKind::List(items) | SyntaxKind::Array(items) = &syntax.kind {
            return Ok(items.is_empty());
        }
    }
    Err(error_val(
        "type-error",
        format!("expected collection type, got {}", val.type_name()),
    ))
}

/// Element/key/grapheme/byte count.
pub fn coll_len(val: &Value) -> Result<usize, Value> {
    if val.is_nil() || val.is_empty_list() {
        return Ok(0);
    }
    if val.is_pair() {
        let vec = val
            .list_to_vec()
            .map_err(|e| error_val("type-error", e.to_string()))?;
        return Ok(vec.len());
    }
    if let Some(elems) = val.as_array() {
        return Ok(elems.len());
    }
    if let Some(arr) = val.as_array_mut() {
        return Ok(arr.borrow().len());
    }
    if let Some(r) = val.with_string(|s| s.graphemes(true).count()) {
        return Ok(r);
    }
    if let Some(buf_ref) = val.as_string_mut() {
        let borrowed = buf_ref.borrow();
        match std::str::from_utf8(&borrowed) {
            Ok(s) => return Ok(s.graphemes(true).count()),
            Err(e) => {
                return Err(error_val(
                    "encoding-error",
                    format!("@string contains invalid UTF-8: {}", e),
                ))
            }
        }
    }
    if let Some(b) = val.as_bytes() {
        return Ok(b.len());
    }
    if let Some(blob_ref) = val.as_bytes_mut() {
        return Ok(blob_ref.borrow().len());
    }
    if let Some(s) = val.as_set() {
        return Ok(s.len());
    }
    if let Some(s) = val.as_set_mut() {
        return Ok(s.borrow().len());
    }
    if let Some(s) = val.as_struct() {
        return Ok(s.len());
    }
    if let Some(t) = val.as_struct_mut() {
        return Ok(t.borrow().len());
    }
    if let Some(sid) = val.as_symbol() {
        if let Some(name) = crate::context::resolve_symbol_name(sid) {
            return Ok(name.graphemes(true).count());
        }
        return Err(error_val(
            "internal-error",
            format!("unable to resolve symbol name for id {:?}", sid),
        ));
    }
    if let Some(name) = val.as_keyword_name() {
        return Ok(name.graphemes(true).count());
    }
    if let Some(syntax) = val.as_syntax() {
        use crate::syntax::SyntaxKind;
        if let SyntaxKind::List(items) | SyntaxKind::Array(items) = &syntax.kind {
            return Ok(items.len());
        }
    }
    Err(error_val(
        "type-error",
        format!("expected collection type, got {}", val.type_name()),
    ))
}

/// Membership test: element in seq/set, key in struct, substring in string.
pub fn coll_has(coll: &Value, needle: &Value) -> Result<bool, Value> {
    // Sets
    let frozen = freeze_value(*needle);
    if let Some(s) = coll.as_set() {
        return Ok(s.binary_search(&frozen).is_ok());
    }
    if let Some(s) = coll.as_set_mut() {
        return Ok(s.borrow().contains(&frozen));
    }
    // Strings — substring check
    if coll.is_string() {
        let needle_str = needle.with_string(|s| s.to_string()).ok_or_else(|| {
            error_val(
                "type-error",
                format!(
                    "has?: expected string as substring, got {}",
                    needle.type_name()
                ),
            )
        })?;
        return coll
            .with_string(|haystack| haystack.contains(&*needle_str))
            .ok_or_else(|| error_val("internal-error", "has?: unreachable string case"));
    }
    if let Some(buf_ref) = coll.as_string_mut() {
        let needle_str = needle.with_string(|s| s.to_string()).ok_or_else(|| {
            error_val(
                "type-error",
                format!(
                    "has?: expected string as substring, got {}",
                    needle.type_name()
                ),
            )
        })?;
        let borrowed = buf_ref.borrow();
        let haystack = String::from_utf8(borrowed.clone()).map_err(|e| {
            error_val(
                "encoding-error",
                format!("has?: buffer contains invalid UTF-8: {}", e),
            )
        })?;
        return Ok(haystack.contains(&*needle_str));
    }
    // Structs — key lookup
    if coll.is_struct() || coll.is_struct_mut() {
        let key = TableKey::from_value(needle).ok_or_else(|| {
            error_val(
                "type-error",
                format!("struct keys must be immutable (got {})", needle.type_name()),
            )
        })?;
        if let Some(s) = coll.as_struct() {
            return Ok(sorted_struct_contains(s, &key));
        }
        if let Some(t) = coll.as_struct_mut() {
            return Ok(t.borrow().contains_key(&key));
        }
    }
    Err(error_val(
        "type-error",
        format!(
            "has?: expected struct, set, or string, got {}",
            coll.type_name()
        ),
    ))
}

/// Collect all elements as `Vec<Value>`.
pub fn coll_to_vec(val: &Value) -> Result<Vec<Value>, Value> {
    // List
    if val.is_pair() || val.is_empty_list() {
        let mut elements = Vec::new();
        let mut cur = *val;
        while let Some(c) = cur.as_pair() {
            elements.push(c.first);
            cur = c.rest;
        }
        return Ok(elements);
    }
    // Array / @array
    if let Some(elems) = val.as_array() {
        return Ok(elems.to_vec());
    }
    if let Some(data) = val.as_array_mut() {
        return Ok(data.borrow().clone());
    }
    // Set / @set
    if let Some(set) = val.as_set() {
        return Ok(set.to_vec());
    }
    if let Some(set) = val.as_set_mut() {
        return Ok(set.borrow().iter().copied().collect());
    }
    // String — grapheme clusters
    if val.is_string() {
        return val
            .with_string(|s| Ok(s.graphemes(true).map(Value::string).collect()))
            .unwrap_or_else(|| Ok(vec![]));
    }
    // @string — grapheme clusters
    if val.is_string_mut() {
        if let Some(data) = val.as_string_mut() {
            let bytes = data.borrow();
            if let Ok(s) = std::str::from_utf8(&bytes) {
                return Ok(s.graphemes(true).map(Value::string).collect());
            }
        }
        return Ok(vec![]);
    }
    // Bytes — each byte as integer
    if let Some(b) = val.as_bytes() {
        return Ok(b.iter().map(|&byte| Value::int(byte as i64)).collect());
    }
    // @bytes
    if let Some(data) = val.as_bytes_mut() {
        return Ok(data
            .borrow()
            .iter()
            .map(|&byte| Value::int(byte as i64))
            .collect());
    }
    // Struct — key-value pairs as 2-element arrays
    if let Some(s) = val.as_struct() {
        return Ok(s
            .iter()
            .map(|(k, v)| Value::array(vec![k.to_value(), *v]))
            .collect());
    }
    if let Some(t) = val.as_struct_mut() {
        return Ok(t
            .borrow()
            .iter()
            .map(|(k, v)| Value::array(vec![k.to_value(), *v]))
            .collect());
    }
    Err(error_val(
        "type-error",
        format!("expected collection, got {}", val.type_name()),
    ))
}

/// Combine two collections (concat for seqs, union for sets, merge for structs).
pub fn coll_combine(a: &Value, b: &Value) -> Result<Value, Value> {
    // Sets — union
    if let (Some(sa), Some(sb)) = (a.as_set(), b.as_set()) {
        let mut result: std::collections::BTreeSet<Value> = sa.iter().copied().collect();
        result.extend(sb.iter().copied());
        return Ok(Value::set(result));
    }
    if let (Some(sa), Some(sb)) = (a.as_set_mut(), b.as_set_mut()) {
        let result: std::collections::BTreeSet<Value> =
            sa.borrow().union(&*sb.borrow()).copied().collect();
        return Ok(Value::set_mut(result));
    }

    // Structs — merge (right wins)
    if let (Some(sa), Some(sb)) = (a.as_struct(), b.as_struct()) {
        let mut result = std::collections::BTreeMap::new();
        result.extend(sa.iter().map(|(k, v)| (k.clone(), *v)));
        result.extend(sb.iter().map(|(k, v)| (k.clone(), *v)));
        return Ok(Value::struct_from(result));
    }
    if let (Some(ta), Some(tb)) = (a.as_struct_mut(), b.as_struct_mut()) {
        let mut result = std::collections::BTreeMap::new();
        result.extend(ta.borrow().iter().map(|(k, v)| (k.clone(), *v)));
        result.extend(tb.borrow().iter().map(|(k, v)| (k.clone(), *v)));
        return Ok(Value::struct_mut_from(result));
    }

    // Lists
    if (a.is_pair() || a.is_empty_list()) && (b.is_pair() || b.is_empty_list()) {
        let mut first = a
            .list_to_vec()
            .map_err(|e| error_val("type-error", e.to_string()))?;
        let second = b
            .list_to_vec()
            .map_err(|e| error_val("type-error", e.to_string()))?;
        first.extend(second);
        let mut result = Value::EMPTY_LIST;
        for val in first.into_iter().rev() {
            result = Value::pair(val, result);
        }
        return Ok(result);
    }

    // Arrays
    if let (Some(ea), Some(eb)) = (a.as_array(), b.as_array()) {
        let mut result = ea.to_vec();
        result.extend(eb.iter().cloned());
        return Ok(Value::array(result));
    }
    if let (Some(ra), Some(rb)) = (a.as_array_mut(), b.as_array_mut()) {
        let mut result = ra.borrow().clone();
        result.extend(rb.borrow().iter().cloned());
        return Ok(Value::array_mut(result));
    }

    // Strings
    if a.is_string() && b.is_string() {
        let mut sa = a.with_string(|s| s.to_string()).unwrap();
        b.with_string(|s| sa.push_str(s));
        return Ok(Value::string(sa.as_str()));
    }
    if a.as_string_mut().is_some() && b.as_string_mut().is_some() {
        let ba = a.as_string_mut().unwrap();
        let bb = b.as_string_mut().unwrap();
        let mut result = ba.borrow().clone();
        result.extend(bb.borrow().iter());
        return Ok(Value::string_mut(result));
    }

    // Bytes
    if let (Some(ba), Some(bb)) = (a.as_bytes(), b.as_bytes()) {
        let mut result = ba.to_vec();
        result.extend(bb.iter());
        return Ok(Value::bytes(result));
    }
    if let (Some(ra), Some(rb)) = (a.as_bytes_mut(), b.as_bytes_mut()) {
        let mut result = ra.borrow().clone();
        result.extend(rb.borrow().iter());
        return Ok(Value::bytes_mut(result));
    }

    Err(error_val(
        "type-error",
        format!("cannot combine {} and {}", a.type_name(), b.type_name()),
    ))
}
