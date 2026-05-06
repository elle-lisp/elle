//! Seq protocol: centralized dispatch for ordered, indexable sequences.
//!
//! Seq extends Collection — these operations apply only to types with a
//! defined element order: list, (), array, @array, string, @string,
//! bytes, @bytes.  Not sets or structs (unordered).
use crate::value::fiberheap;
use crate::value::{error_val, list, Value};
use unicode_segmentation::UnicodeSegmentation;

use super::access::{resolve_index, resolve_slice_index};

const SEQ_TYPES: &str = "sequence (list, array, string, bytes)";

fn seq_type_error(op: &str, val: &Value) -> Value {
    error_val(
        "type-error",
        format!("{}: expected {}, got {}", op, SEQ_TYPES, val.type_name()),
    )
}

/// Get the first element of a sequence.
pub fn seq_first(val: &Value) -> Result<Value, Value> {
    if let Some(pair) = val.as_pair() {
        return Ok(pair.first);
    }
    if val.is_empty_list() {
        return Err(error_val("argument-error", "first: empty sequence"));
    }
    if let Some(elems) = val.as_array() {
        return elems
            .first()
            .copied()
            .ok_or_else(|| error_val("argument-error", "first: empty sequence"));
    }
    if let Some(arr) = val.as_array_mut() {
        let borrowed = arr.borrow();
        return borrowed
            .first()
            .copied()
            .ok_or_else(|| error_val("argument-error", "first: empty sequence"));
    }
    if let Some(result) = val.with_string(|s| match s.graphemes(true).next() {
        Some(g) => Ok(Value::string(g)),
        None => Err(error_val("argument-error", "first: empty sequence")),
    }) {
        return result;
    }
    if let Some(buf_ref) = val.as_string_mut() {
        let borrowed = buf_ref.borrow();
        if let Ok(s) = std::str::from_utf8(&borrowed) {
            return match s.graphemes(true).next() {
                Some(g) => Ok(Value::string(g)),
                None => Err(error_val("argument-error", "first: empty sequence")),
            };
        }
        return Err(error_val("argument-error", "first: empty sequence"));
    }
    if let Some(b) = val.as_bytes() {
        return if b.is_empty() {
            Err(error_val("argument-error", "first: empty sequence"))
        } else {
            Ok(Value::int(b[0] as i64))
        };
    }
    if let Some(blob_ref) = val.as_bytes_mut() {
        let borrowed = blob_ref.borrow();
        return if borrowed.is_empty() {
            Err(error_val("argument-error", "first: empty sequence"))
        } else {
            Ok(Value::int(borrowed[0] as i64))
        };
    }
    Err(seq_type_error("first", val))
}

/// Get the rest of a sequence (type-preserving).
pub fn seq_rest(val: &Value) -> Result<Value, Value> {
    if let Some(pair) = val.as_pair() {
        return Ok(pair.rest);
    }
    if val.is_empty_list() {
        return Ok(Value::EMPTY_LIST);
    }
    if let Some(elems) = val.as_array() {
        return Ok(if elems.len() <= 1 {
            Value::array(vec![])
        } else {
            Value::array(elems[1..].to_vec())
        });
    }
    if let Some(arr) = val.as_array_mut() {
        let borrowed = arr.borrow();
        return Ok(if borrowed.len() <= 1 {
            Value::array_mut(vec![])
        } else {
            Value::array_mut(borrowed[1..].to_vec())
        });
    }
    if let Some(result) = val.with_string(|s| {
        let rest: String = s.graphemes(true).skip(1).collect();
        Value::string(rest)
    }) {
        return Ok(result);
    }
    if let Some(buf_ref) = val.as_string_mut() {
        let borrowed = buf_ref.borrow();
        if let Ok(s) = std::str::from_utf8(&borrowed) {
            let rest: String = s.graphemes(true).skip(1).collect();
            return Ok(Value::string_mut(rest.into_bytes()));
        }
        return Ok(Value::string_mut(vec![]));
    }
    if let Some(b) = val.as_bytes() {
        return Ok(if b.len() <= 1 {
            Value::bytes(vec![])
        } else {
            Value::bytes(b[1..].to_vec())
        });
    }
    if let Some(blob_ref) = val.as_bytes_mut() {
        let borrowed = blob_ref.borrow();
        return Ok(if borrowed.len() <= 1 {
            Value::bytes_mut(vec![])
        } else {
            Value::bytes_mut(borrowed[1..].to_vec())
        });
    }
    Err(seq_type_error("rest", val))
}

/// Get the last element of a sequence.
pub fn seq_last(val: &Value) -> Result<Value, Value> {
    if val.is_empty_list() {
        return Err(error_val("argument-error", "last: empty sequence"));
    }
    if val.is_pair() {
        let mut current = *val;
        let mut last = Value::NIL;
        while let Some(pair) = current.as_pair() {
            last = pair.first;
            current = pair.rest;
        }
        return Ok(last);
    }
    if let Some(elems) = val.as_array() {
        return elems
            .last()
            .copied()
            .ok_or_else(|| error_val("argument-error", "last: empty sequence"));
    }
    if let Some(arr) = val.as_array_mut() {
        let borrowed = arr.borrow();
        return borrowed
            .last()
            .copied()
            .ok_or_else(|| error_val("argument-error", "last: empty sequence"));
    }
    if let Some(result) = val.with_string(|s| match s.graphemes(true).next_back() {
        Some(g) => Ok(Value::string(g)),
        None => Err(error_val("argument-error", "last: empty sequence")),
    }) {
        return result;
    }
    if let Some(buf_ref) = val.as_string_mut() {
        let borrowed = buf_ref.borrow();
        if let Ok(s) = std::str::from_utf8(&borrowed) {
            return match s.graphemes(true).next_back() {
                Some(g) => Ok(Value::string(g)),
                None => Err(error_val("argument-error", "last: empty sequence")),
            };
        }
        return Err(error_val("argument-error", "last: empty sequence"));
    }
    if let Some(b) = val.as_bytes() {
        return b
            .last()
            .map(|&byte| Value::int(byte as i64))
            .ok_or_else(|| error_val("argument-error", "last: empty sequence"));
    }
    if let Some(blob_ref) = val.as_bytes_mut() {
        let borrowed = blob_ref.borrow();
        return borrowed
            .last()
            .map(|&byte| Value::int(byte as i64))
            .ok_or_else(|| error_val("argument-error", "last: empty sequence"));
    }
    Err(seq_type_error("last", val))
}

/// Get element at index n.
pub fn seq_nth(val: &Value, n: i64) -> Result<Value, Value> {
    if val.is_pair() {
        // Positive index: walk forward
        if n >= 0 {
            let mut current = *val;
            let mut i = 0usize;
            loop {
                if current.is_empty_list() || current.is_nil() {
                    return Err(error_val(
                        "argument-error",
                        format!("nth: index {} out of bounds", n),
                    ));
                }
                if let Some(p) = current.as_pair() {
                    if i == n as usize {
                        return Ok(p.first);
                    }
                    current = p.rest;
                    i += 1;
                } else {
                    return Err(error_val(
                        "argument-error",
                        format!("nth: index {} out of bounds", n),
                    ));
                }
            }
        } else {
            // Negative: need length
            let mut len = 0usize;
            let mut cur = *val;
            while let Some(c) = cur.as_pair() {
                len += 1;
                cur = c.rest;
            }
            let resolved = n + len as i64;
            if resolved < 0 {
                return Err(error_val(
                    "argument-error",
                    format!("nth: index {} out of bounds (length {})", n, len),
                ));
            }
            return seq_nth(val, resolved);
        }
    }
    if val.is_empty_list() {
        return Err(error_val(
            "argument-error",
            format!("nth: index {} out of bounds (empty list)", n),
        ));
    }
    if let Some(elems) = val.as_array() {
        return resolve_index(n, elems.len())
            .map(|i| elems[i])
            .ok_or_else(|| {
                error_val(
                    "argument-error",
                    format!("nth: index {} out of bounds (length {})", n, elems.len()),
                )
            });
    }
    if let Some(arr) = val.as_array_mut() {
        let borrowed = arr.borrow();
        return resolve_index(n, borrowed.len())
            .map(|i| borrowed[i])
            .ok_or_else(|| {
                error_val(
                    "argument-error",
                    format!("nth: index {} out of bounds (length {})", n, borrowed.len()),
                )
            });
    }
    if let Some(result) = val.with_string(|s| {
        let graphemes: Vec<&str> = s.graphemes(true).collect();
        resolve_index(n, graphemes.len())
            .map(|i| Value::string(graphemes[i]))
            .ok_or_else(|| {
                error_val(
                    "argument-error",
                    format!(
                        "nth: index {} out of bounds (length {})",
                        n,
                        graphemes.len()
                    ),
                )
            })
    }) {
        return result;
    }
    if let Some(buf_ref) = val.as_string_mut() {
        let borrowed = buf_ref.borrow();
        if let Ok(s) = std::str::from_utf8(&borrowed) {
            let graphemes: Vec<&str> = s.graphemes(true).collect();
            return resolve_index(n, graphemes.len())
                .map(|i| Value::string(graphemes[i]))
                .ok_or_else(|| {
                    error_val(
                        "argument-error",
                        format!(
                            "nth: index {} out of bounds (length {})",
                            n,
                            graphemes.len()
                        ),
                    )
                });
        }
        return Err(error_val(
            "argument-error",
            format!("nth: index {} out of bounds (empty @string)", n),
        ));
    }
    if let Some(b) = val.as_bytes() {
        return resolve_index(n, b.len())
            .map(|i| Value::int(b[i] as i64))
            .ok_or_else(|| {
                error_val(
                    "argument-error",
                    format!("nth: index {} out of bounds (length {})", n, b.len()),
                )
            });
    }
    if let Some(blob_ref) = val.as_bytes_mut() {
        let borrowed = blob_ref.borrow();
        return resolve_index(n, borrowed.len())
            .map(|i| Value::int(borrowed[i] as i64))
            .ok_or_else(|| {
                error_val(
                    "argument-error",
                    format!("nth: index {} out of bounds (length {})", n, borrowed.len()),
                )
            });
    }
    Err(seq_type_error("nth", val))
}

/// All elements except the last (type-preserving).
pub fn seq_butlast(val: &Value) -> Result<Value, Value> {
    if val.is_empty_list() {
        return Ok(Value::EMPTY_LIST);
    }
    if val.is_pair() {
        let vec = val
            .list_to_vec()
            .map_err(|e| error_val("type-error", e.to_string()))?;
        if vec.is_empty() {
            return Ok(Value::EMPTY_LIST);
        }
        return Ok(list(vec[..vec.len() - 1].to_vec()));
    }
    if let Some(elems) = val.as_array() {
        return Ok(if elems.is_empty() {
            Value::array(vec![])
        } else {
            Value::array(elems[..elems.len() - 1].to_vec())
        });
    }
    if let Some(arr) = val.as_array_mut() {
        let borrowed = arr.borrow();
        return Ok(if borrowed.is_empty() {
            Value::array_mut(vec![])
        } else {
            Value::array_mut(borrowed[..borrowed.len() - 1].to_vec())
        });
    }
    if let Some(result) = val.with_string(|s| {
        let graphemes: Vec<&str> = s.graphemes(true).collect();
        if graphemes.is_empty() {
            Value::string("")
        } else {
            Value::string(graphemes[..graphemes.len() - 1].concat())
        }
    }) {
        return Ok(result);
    }
    if let Some(buf_ref) = val.as_string_mut() {
        let borrowed = buf_ref.borrow();
        if let Ok(s) = std::str::from_utf8(&borrowed) {
            let graphemes: Vec<&str> = s.graphemes(true).collect();
            return Ok(if graphemes.is_empty() {
                Value::string_mut(vec![])
            } else {
                Value::string_mut(graphemes[..graphemes.len() - 1].concat().into_bytes())
            });
        }
        return Ok(Value::string_mut(vec![]));
    }
    if let Some(b) = val.as_bytes() {
        return Ok(if b.is_empty() {
            Value::bytes(vec![])
        } else {
            Value::bytes(b[..b.len() - 1].to_vec())
        });
    }
    if let Some(blob_ref) = val.as_bytes_mut() {
        let borrowed = blob_ref.borrow();
        return Ok(if borrowed.is_empty() {
            Value::bytes_mut(vec![])
        } else {
            Value::bytes_mut(borrowed[..borrowed.len() - 1].to_vec())
        });
    }
    Err(seq_type_error("butlast", val))
}

/// Reverse a sequence (type-preserving).
pub fn seq_reverse(val: &Value) -> Result<Value, Value> {
    if val.is_empty_list() {
        return Ok(Value::EMPTY_LIST);
    }
    if val.is_pair() {
        let mut vec = val
            .list_to_vec()
            .map_err(|e| error_val("type-error", e.to_string()))?;
        vec.reverse();
        return Ok(list(vec));
    }
    if let Some(elems) = val.as_array() {
        let mut vec = elems.to_vec();
        vec.reverse();
        return Ok(Value::array(vec));
    }
    if let Some(arr) = val.as_array_mut() {
        let mut vec = arr.borrow().to_vec();
        vec.reverse();
        return Ok(Value::array_mut(vec));
    }
    if let Some(result) = val.with_string(|s| {
        let reversed: String = s.graphemes(true).rev().collect();
        Value::string(reversed)
    }) {
        return Ok(result);
    }
    if let Some(buf_ref) = val.as_string_mut() {
        let borrowed = buf_ref.borrow();
        if let Ok(s) = std::str::from_utf8(&borrowed) {
            let reversed: String = s.graphemes(true).rev().collect();
            return Ok(Value::string_mut(reversed.into_bytes()));
        }
        return Ok(Value::string_mut(vec![]));
    }
    if let Some(b) = val.as_bytes() {
        let mut vec = b.to_vec();
        vec.reverse();
        return Ok(Value::bytes(vec));
    }
    if let Some(blob_ref) = val.as_bytes_mut() {
        let mut vec = blob_ref.borrow().to_vec();
        vec.reverse();
        return Ok(Value::bytes_mut(vec));
    }
    Err(seq_type_error("reverse", val))
}

/// Slice a sequence from start to end (type-preserving).
pub fn seq_slice(val: &Value, start: i64, end: i64) -> Result<Value, Value> {
    // Bytes
    if let Some(b) = val.as_bytes() {
        let s = resolve_slice_index(start, b.len());
        let e = resolve_slice_index(end, b.len());
        return Ok(if s >= e {
            Value::bytes(vec![])
        } else {
            Value::bytes(b[s..e].to_vec())
        });
    }
    if let Some(blob_ref) = val.as_bytes_mut() {
        let borrowed = blob_ref.borrow();
        let s = resolve_slice_index(start, borrowed.len());
        let e = resolve_slice_index(end, borrowed.len());
        return Ok(if s >= e {
            Value::bytes_mut(vec![])
        } else {
            Value::bytes_mut(borrowed[s..e].to_vec())
        });
    }
    // Arrays
    if let Some(elems) = val.as_array() {
        let s = resolve_slice_index(start, elems.len());
        let e = resolve_slice_index(end, elems.len());
        return Ok(if s >= e {
            Value::array(vec![])
        } else {
            Value::array(elems[s..e].to_vec())
        });
    }
    if let Some(arr_ref) = val.as_array_mut() {
        let borrowed = arr_ref.borrow();
        let s = resolve_slice_index(start, borrowed.len());
        let e = resolve_slice_index(end, borrowed.len());
        return Ok(if s >= e {
            Value::array_mut(vec![])
        } else {
            Value::array_mut(borrowed[s..e].to_vec())
        });
    }
    // Strings (grapheme-aware)
    if val.is_string() {
        return val
            .with_string(|str_val| {
                let count = str_val.graphemes(true).count();
                let s = resolve_slice_index(start, count);
                let e = resolve_slice_index(end, count);
                Ok(slice_graphemes_immut(str_val, s, e))
            })
            .unwrap();
    }
    if let Some(buf_ref) = val.as_string_mut() {
        let borrowed = buf_ref.borrow();
        let str_val = unsafe { std::str::from_utf8_unchecked(&borrowed) };
        let count = str_val.graphemes(true).count();
        let s = resolve_slice_index(start, count);
        let e = resolve_slice_index(end, count);
        return Ok(slice_graphemes_mut(str_val, s, e));
    }
    // Lists
    if val.is_empty_list() || val.is_pair() {
        let elems = val
            .list_to_vec()
            .map_err(|e| error_val("type-error", e.to_string()))?;
        let s = resolve_slice_index(start, elems.len());
        let e = resolve_slice_index(end, elems.len());
        if s >= e {
            return Ok(Value::EMPTY_LIST);
        }
        let mut result = Value::EMPTY_LIST;
        for v in elems[s..e].iter().rev() {
            result = Value::pair(*v, result);
        }
        return Ok(result);
    }
    Err(seq_type_error("slice", val))
}

fn slice_graphemes_immut(s: &str, start: usize, end: usize) -> Value {
    let graphemes: Vec<&str> = s.graphemes(true).collect();
    let cs = start.min(graphemes.len());
    let ce = end.min(graphemes.len());
    if cs >= ce {
        Value::string("")
    } else {
        Value::string(graphemes[cs..ce].concat())
    }
}

fn slice_graphemes_mut(s: &str, start: usize, end: usize) -> Value {
    let graphemes: Vec<&str> = s.graphemes(true).collect();
    let cs = start.min(graphemes.len());
    let ce = end.min(graphemes.len());
    if cs >= ce {
        Value::string_mut(vec![])
    } else {
        Value::string_mut(graphemes[cs..ce].concat().into_bytes())
    }
}

/// Sort a sequence (type-preserving).
pub fn seq_sort(val: &Value) -> Result<Value, Value> {
    if let Some(arr) = val.as_array_mut() {
        arr.borrow_mut().sort();
        return Ok(*val);
    }
    if let Some(elems) = val.as_array() {
        let mut vec = elems.to_vec();
        vec.sort();
        return Ok(Value::array(vec));
    }
    if val.is_empty_list() {
        return Ok(Value::EMPTY_LIST);
    }
    if val.is_pair() {
        let mut vec = val
            .list_to_vec()
            .map_err(|e| error_val("type-error", e.to_string()))?;
        vec.sort();
        return Ok(list(vec));
    }
    Err(error_val(
        "type-error",
        format!(
            "sort: expected list, array, or @array, got {}",
            val.type_name()
        ),
    ))
}

/// Push an element onto the end of a sequence (type-aware).
pub fn seq_push(val: &Value, elem: Value) -> Result<Value, Value> {
    // @array — mutate in place
    if let Some(vec_ref) = val.as_array_mut() {
        fiberheap::incref(elem);
        vec_ref.borrow_mut().push(elem);
        return Ok(*val);
    }
    // @string — append string
    if let Some(buf_ref) = val.as_string_mut() {
        let s = elem.with_string(|s| s.to_string()).ok_or_else(|| {
            error_val(
                "type-error",
                format!(
                    "push: @string value must be string, got {}",
                    elem.type_name()
                ),
            )
        })?;
        buf_ref.borrow_mut().extend_from_slice(s.as_bytes());
        return Ok(*val);
    }
    // @bytes — append byte
    if let Some(blob_ref) = val.as_bytes_mut() {
        let byte = match elem.as_int() {
            Some(n) if (0..=255).contains(&n) => n as u8,
            Some(n) => {
                return Err(error_val(
                    "argument-error",
                    format!("push: byte value out of range 0-255: {}", n),
                ))
            }
            None => {
                return Err(error_val(
                    "type-error",
                    format!(
                        "push: @bytes value must be integer, got {}",
                        elem.type_name()
                    ),
                ))
            }
        };
        blob_ref.borrow_mut().push(byte);
        return Ok(*val);
    }
    // Immutable array
    if let Some(elems) = val.as_array() {
        let mut new = elems.to_vec();
        new.push(elem);
        return Ok(Value::array(new));
    }
    // Immutable string
    if val.is_string() {
        let s = elem.with_string(|s| s.to_string()).ok_or_else(|| {
            error_val(
                "type-error",
                format!(
                    "push: string value must be string, got {}",
                    elem.type_name()
                ),
            )
        })?;
        return val
            .with_string(|base| {
                let mut new = base.to_string();
                new.push_str(&s);
                Ok(Value::string(new))
            })
            .unwrap();
    }
    // Immutable bytes
    if let Some(b) = val.as_bytes() {
        let byte = match elem.as_int() {
            Some(n) if (0..=255).contains(&n) => n as u8,
            Some(n) => {
                return Err(error_val(
                    "argument-error",
                    format!("push: byte value out of range 0-255: {}", n),
                ))
            }
            None => {
                return Err(error_val(
                    "type-error",
                    format!(
                        "push: bytes value must be integer, got {}",
                        elem.type_name()
                    ),
                ))
            }
        };
        let mut new = b.to_vec();
        new.push(byte);
        return Ok(Value::bytes(new));
    }
    Err(error_val(
        "type-error",
        format!(
            "push: expected array, @array, string, @string, bytes, or @bytes, got {}",
            val.type_name()
        ),
    ))
}

/// Pop the last element from a mutable sequence.
pub fn seq_pop(val: &Value) -> Result<Value, Value> {
    if let Some(vec_ref) = val.as_array_mut() {
        let mut vec = vec_ref.borrow_mut();
        match vec.pop() {
            Some(v) => {
                drop(vec);
                fiberheap::decref(v);
                return Ok(v);
            }
            None => return Err(error_val("argument-error", "pop: empty array")),
        }
    }
    if let Some(buf_ref) = val.as_string_mut() {
        let mut buf = buf_ref.borrow_mut();
        if buf.is_empty() {
            return Err(error_val("argument-error", "pop: empty @string"));
        }
        let s = std::str::from_utf8(&buf)
            .map_err(|_| error_val("encoding-error", "pop: @string contains invalid UTF-8"))?;
        let cluster = s.graphemes(true).next_back().unwrap().to_string();
        let new_len = buf.len() - cluster.len();
        buf.truncate(new_len);
        drop(buf);
        return Ok(Value::string(cluster));
    }
    if let Some(blob_ref) = val.as_bytes_mut() {
        let mut blob = blob_ref.borrow_mut();
        match blob.pop() {
            Some(byte) => {
                drop(blob);
                return Ok(Value::int(byte as i64));
            }
            None => return Err(error_val("argument-error", "pop: empty @bytes")),
        }
    }
    Err(error_val(
        "type-error",
        format!(
            "pop: expected @array, @string, or @bytes, got {}",
            val.type_name()
        ),
    ))
}
