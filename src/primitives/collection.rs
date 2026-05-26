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
/// Collect set elements into a BTreeSet regardless of mutability.
pub fn set_elements(v: &Value) -> Option<std::collections::BTreeSet<Value>> {
    v.as_set()
        .map(|s| s.iter().copied().collect())
        .or_else(|| v.as_set_mut().map(|s| s.borrow().iter().copied().collect()))
}

/// Collect struct entries into a BTreeMap regardless of mutability.
fn struct_entries(
    v: &Value,
) -> Option<std::collections::BTreeMap<crate::value::heap::TableKey, Value>> {
    v.as_struct()
        .map(|s| s.iter().map(|(k, v)| (k.clone(), *v)).collect())
        .or_else(|| {
            v.as_struct_mut()
                .map(|s| s.borrow().iter().map(|(k, v)| (k.clone(), *v)).collect())
        })
}

/// Collect array elements into a Vec regardless of mutability.
fn array_elements(v: &Value) -> Option<Vec<Value>> {
    v.as_array()
        .map(|a| a.to_vec())
        .or_else(|| v.as_array_mut().map(|a| a.borrow().clone()))
}

/// Collect string content regardless of mutability.
fn string_content(v: &Value) -> Option<String> {
    v.with_string(|s| s.to_string()).or_else(|| {
        v.as_string_mut()
            .map(|s| String::from_utf8_lossy(&s.borrow()).into_owned())
    })
}

/// Collect bytes content regardless of mutability.
fn bytes_content(v: &Value) -> Option<Vec<u8>> {
    v.as_bytes()
        .map(|b| b.to_vec())
        .or_else(|| v.as_bytes_mut().map(|b| b.borrow().clone()))
}

/// Is the value a mutable variant of its base type?
pub fn is_mutable(v: &Value) -> bool {
    v.as_set_mut().is_some()
        || v.as_struct_mut().is_some()
        || v.as_array_mut().is_some()
        || v.as_string_mut().is_some()
        || v.as_bytes_mut().is_some()
}

pub fn coll_combine(a: &Value, b: &Value) -> Result<Value, Value> {
    let a_mut = is_mutable(a);

    // Sets — union
    if let (Some(sa), Some(sb)) = (set_elements(a), set_elements(b)) {
        let result: std::collections::BTreeSet<Value> = sa.union(&sb).copied().collect();
        return Ok(if a_mut {
            Value::set_mut(result)
        } else {
            Value::set(result)
        });
    }

    // Structs — merge (right wins)
    if let (Some(mut ea), Some(eb)) = (struct_entries(a), struct_entries(b)) {
        ea.extend(eb);
        return Ok(if a_mut {
            Value::struct_mut_from(ea)
        } else {
            Value::struct_from(ea)
        });
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
    if let (Some(mut ea), Some(eb)) = (array_elements(a), array_elements(b)) {
        ea.extend(eb);
        return Ok(if a_mut {
            Value::array_mut(ea)
        } else {
            Value::array(ea)
        });
    }

    // Strings
    if let (Some(mut sa), Some(sb)) = (string_content(a), string_content(b)) {
        sa.push_str(&sb);
        return Ok(if a_mut {
            Value::string_mut(sa.into_bytes())
        } else {
            Value::string(sa.as_str())
        });
    }

    // Bytes
    if let (Some(mut ba), Some(bb)) = (bytes_content(a), bytes_content(b)) {
        ba.extend(bb);
        return Ok(if a_mut {
            Value::bytes_mut(ba)
        } else {
            Value::bytes(ba)
        });
    }

    Err(error_val(
        "type-error",
        format!("cannot combine {} and {}", a.type_name(), b.type_name()),
    ))
}
