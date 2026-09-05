use crate::value::{TableKey, Value};

/// The refusal for a keyword whose spelling neither the calling instance's
/// memo nor the static vocabulary carries.
///
/// JSON has no rendering for a name hash: `"0xcbf2…"` would read back as a
/// different name, so there is no fallback to take. The message names the
/// hash because that is the author's only handle on which keyword it was —
/// and, upstream, on the mint site that never recorded the spelling
/// (docs/impl/symbol.md § "A spelling the runtime itself mints").
fn unspellable(what: &str, hash: u64) -> String {
    format!("{} has no learned spelling: #<keyword:{:#x}>", what, hash)
}

/// A struct key as a quoted JSON object key. `container` names the struct
/// kind, for the message a key of neither JSON-writable type earns.
fn json_object_key(
    key: &TableKey,
    symbols: Option<&crate::symbol::SymbolTable>,
    container: &str,
) -> Result<String, String> {
    match key {
        TableKey::String(v) => Ok(escape_json_string(
            v.as_str().expect("a string key holds a string"),
        )),
        TableKey::Keyword(hash) => {
            match crate::value::keyword::resolve_keyword_name(symbols, *hash) {
                Some(name) => Ok(escape_json_string(name)),
                None => Err(unspellable("keyword struct key", *hash)),
            }
        }
        _ => Err(format!(
            "{} keys must be strings or keywords for JSON serialization",
            container
        )),
    }
}

/// A keyword as a quoted JSON string.
fn json_keyword(hash: u64, symbols: Option<&crate::symbol::SymbolTable>) -> Result<String, String> {
    crate::value::keyword::resolve_keyword_name(symbols, hash)
        .map(escape_json_string)
        .ok_or_else(|| unspellable("keyword", hash))
}

/// Escape a string for JSON output
pub fn escape_json_string(s: &str) -> String {
    let mut result = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\u{0008}' => result.push_str("\\b"),
            '\u{000C}' => result.push_str("\\f"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c.is_control() => {
                result.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => result.push(c),
        }
    }
    result.push('"');
    result
}

/// Serialize a value to compact JSON
pub fn serialize_value(
    value: &Value,
    symbols: Option<&crate::symbol::SymbolTable>,
) -> Result<String, String> {
    if value.is_nil() {
        Ok("null".to_string())
    } else if let Some(b) = value.as_bool() {
        Ok(if b { "true" } else { "false" }.to_string())
    } else if let Some(i) = value.as_int() {
        Ok(i.to_string())
    } else if let Some(f) = value.as_float() {
        // Guard against non-finite values
        if f.is_nan() || f.is_infinite() {
            return Err("Cannot serialize non-finite float value to JSON".to_string());
        }
        // Ensure floats always have a decimal point
        let s = f.to_string();
        if s.contains('.') || s.contains('e') || s.contains('E') {
            Ok(s)
        } else {
            Ok(format!("{}.0", s))
        }
    } else if let Some(r) = value.with_string(escape_json_string) {
        Ok(r)
    } else if value.is_empty_list() {
        Ok("[]".to_string())
    } else if value.is_pair() {
        // Convert list to array
        let vec = value.list_to_vec()?;
        let elements: Result<Vec<String>, String> =
            vec.iter().map(|v| serialize_value(v, symbols)).collect();
        Ok(format!("[{}]", elements?.join(",")))
    } else if let Some(v) = value.as_array_mut() {
        let borrowed = v.borrow();
        let elements: Result<Vec<String>, String> = borrowed
            .iter()
            .map(|v| serialize_value(v, symbols))
            .collect();
        Ok(format!("[{}]", elements?.join(",")))
    } else if let Some(t) = value.as_struct_mut() {
        let mstruct = t.borrow();
        let mut pairs = Vec::new();
        for (k, v) in mstruct.iter() {
            let key_str = json_object_key(k, symbols, "@struct")?;
            let val_str = serialize_value(v, symbols)?;
            pairs.push(format!("{}:{}", key_str, val_str));
        }
        Ok(format!("{{{}}}", pairs.join(",")))
    } else if let Some(s) = value.as_struct() {
        let mut pairs = Vec::new();
        for (k, v) in s.iter() {
            let key_str = json_object_key(k, symbols, "Struct")?;
            let val_str = serialize_value(v, symbols)?;
            pairs.push(format!("{}:{}", key_str, val_str));
        }
        Ok(format!("{{{}}}", pairs.join(",")))
    } else if let Some(hash) = value.keyword_hash() {
        json_keyword(hash, symbols)
    } else if value.is_closure() {
        Err("Cannot serialize closures to JSON".to_string())
    } else if value.is_symbol() {
        Err("Cannot serialize symbols to JSON".to_string())
    } else if let Some(cell) = value.as_lbox() {
        // Dereference the cell and serialize the inner value
        let inner = cell.borrow();
        serialize_value(&inner, symbols)
    } else if let Some(tag) = value.heap_tag() {
        use crate::value::heap::HeapTag;
        match tag {
            HeapTag::LString => Err("String should have been handled above".to_string()),
            HeapTag::Pair => Err("Pair should have been handled above".to_string()),
            HeapTag::LArrayMut => Err("Array should have been handled above".to_string()),
            HeapTag::LStructMut => Err("@struct should have been handled above".to_string()),
            HeapTag::LStruct => Err("Struct should have been handled above".to_string()),
            HeapTag::Closure => Err("Cannot serialize closures to JSON".to_string()),
            HeapTag::LArray => {
                if let Some(elems) = value.as_array() {
                    let items: Result<Vec<String>, String> =
                        elems.iter().map(|v| serialize_value(v, symbols)).collect();
                    Ok(format!("[{}]", items?.join(",")))
                } else {
                    Err("Tuple should have been accessible".to_string())
                }
            }
            HeapTag::LBox => Err("LBox should have been handled above".to_string()),
            HeapTag::CaptureCell => Err("Cannot serialize capture cell to JSON".to_string()),
            HeapTag::Float => {
                // This is a heap-allocated float (for NaN values)
                Err("Cannot serialize non-finite float value to JSON".to_string())
            }
            HeapTag::LibHandle => Err("Cannot serialize library handles to JSON".to_string()),

            HeapTag::ThreadHandle => Err("Cannot serialize thread handles to JSON".to_string()),
            HeapTag::Fiber => Err("Cannot serialize fibers to JSON".to_string()),
            HeapTag::Syntax => Err("Cannot serialize syntax objects to JSON".to_string()),
            HeapTag::FFISignature => Err("Cannot serialize FFI signatures to JSON".to_string()),
            HeapTag::FFIType => Err("Cannot serialize FFI type descriptors to JSON".to_string()),
            HeapTag::ManagedPointer => Err("Cannot serialize pointers to JSON".to_string()),
            HeapTag::LStringMut => Err("Cannot serialize buffers to JSON".to_string()),
            HeapTag::LBytes => Err("Cannot serialize bytes to JSON".to_string()),
            HeapTag::LBytesMut => Err("Cannot serialize blobs to JSON".to_string()),
            HeapTag::External => Err("Cannot serialize external objects to JSON".to_string()),
            HeapTag::Parameter => Err("Cannot serialize parameters to JSON".to_string()),
            HeapTag::LSet => {
                if let Some(s) = value.as_set() {
                    let items: Result<Vec<String>, String> =
                        s.iter().map(|v| serialize_value(v, symbols)).collect();
                    Ok(format!("[{}]", items?.join(",")))
                } else {
                    Err("Set should have been accessible".to_string())
                }
            }
            HeapTag::LSetMut => {
                if let Some(s_ref) = value.as_set_mut() {
                    let s = s_ref.borrow();
                    let items: Result<Vec<String>, String> =
                        s.iter().map(|v| serialize_value(v, symbols)).collect();
                    Ok(format!("[{}]", items?.join(",")))
                } else {
                    Err("Mutable set should have been accessible".to_string())
                }
            }
            HeapTag::ClosureTemplate => {
                Err("Cannot serialize closure templates to JSON".to_string())
            }
        }
    } else {
        Err("Cannot serialize unknown value type to JSON".to_string())
    }
}

/// Serialize a value to pretty-printed JSON with indentation
pub fn serialize_value_pretty(
    value: &Value,
    symbols: Option<&crate::symbol::SymbolTable>,
    indent_level: usize,
) -> Result<String, String> {
    let indent = "  ".repeat(indent_level);
    let next_indent = "  ".repeat(indent_level + 1);

    if value.is_nil() {
        Ok("null".to_string())
    } else if let Some(b) = value.as_bool() {
        Ok(if b { "true" } else { "false" }.to_string())
    } else if let Some(i) = value.as_int() {
        Ok(i.to_string())
    } else if let Some(f) = value.as_float() {
        // Guard against non-finite values
        if f.is_nan() || f.is_infinite() {
            return Err("Cannot serialize non-finite float value to JSON".to_string());
        }
        let s = f.to_string();
        if s.contains('.') || s.contains('e') || s.contains('E') {
            Ok(s)
        } else {
            Ok(format!("{}.0", s))
        }
    } else if let Some(r) = value.with_string(escape_json_string) {
        Ok(r)
    } else if value.is_empty_list() {
        Ok("[]".to_string())
    } else if value.is_pair() {
        let vec = value.list_to_vec()?;
        if vec.is_empty() {
            return Ok("[]".to_string());
        }
        let elements: Result<Vec<String>, String> = vec
            .iter()
            .map(|v| serialize_value_pretty(v, symbols, indent_level + 1))
            .collect();
        Ok(format!(
            "[\n{}{}\n{}]",
            next_indent,
            elements?.join(&format!(",\n{}", next_indent)),
            indent
        ))
    } else if let Some(v) = value.as_array_mut() {
        let borrowed = v.borrow();
        if borrowed.is_empty() {
            return Ok("[]".to_string());
        }
        let elements: Result<Vec<String>, String> = borrowed
            .iter()
            .map(|val| serialize_value_pretty(val, symbols, indent_level + 1))
            .collect();
        Ok(format!(
            "[\n{}{}\n{}]",
            next_indent,
            elements?.join(&format!(",\n{}", next_indent)),
            indent
        ))
    } else if let Some(t) = value.as_struct_mut() {
        let mstruct = t.borrow();
        if mstruct.is_empty() {
            return Ok("{}".to_string());
        }
        let mut pairs = Vec::new();
        for (k, v) in mstruct.iter() {
            let key_str = json_object_key(k, symbols, "@struct")?;
            let val_str = serialize_value_pretty(v, symbols, indent_level + 1)?;
            pairs.push(format!("{}: {}", key_str, val_str));
        }
        Ok(format!(
            "{{\n{}{}\n{}}}",
            next_indent,
            pairs.join(&format!(",\n{}", next_indent)),
            indent
        ))
    } else if let Some(s) = value.as_struct() {
        if s.is_empty() {
            return Ok("{}".to_string());
        }
        let mut pairs = Vec::new();
        for (k, v) in s.iter() {
            let key_str = json_object_key(k, symbols, "Struct")?;
            let val_str = serialize_value_pretty(v, symbols, indent_level + 1)?;
            pairs.push(format!("{}: {}", key_str, val_str));
        }
        Ok(format!(
            "{{\n{}{}\n{}}}",
            next_indent,
            pairs.join(&format!(",\n{}", next_indent)),
            indent
        ))
    } else if let Some(hash) = value.keyword_hash() {
        json_keyword(hash, symbols)
    } else if value.is_closure() {
        Err("Cannot serialize closures to JSON".to_string())
    } else if value.is_symbol() {
        Err("Cannot serialize symbols to JSON".to_string())
    } else if let Some(cell) = value.as_lbox() {
        // Dereference the cell and serialize the inner value
        let inner = cell.borrow();
        serialize_value_pretty(&inner, symbols, indent_level)
    } else if let Some(tag) = value.heap_tag() {
        use crate::value::heap::HeapTag;
        match tag {
            HeapTag::LString => Err("String should have been handled above".to_string()),
            HeapTag::Pair => Err("Pair should have been handled above".to_string()),
            HeapTag::LArrayMut => Err("Array should have been handled above".to_string()),
            HeapTag::LStructMut => Err("@struct should have been handled above".to_string()),
            HeapTag::LStruct => Err("Struct should have been handled above".to_string()),
            HeapTag::Closure => Err("Cannot serialize closures to JSON".to_string()),
            HeapTag::LArray => {
                if let Some(elems) = value.as_array() {
                    let items: Result<Vec<String>, String> = elems
                        .iter()
                        .map(|v| serialize_value_pretty(v, symbols, indent_level + 1))
                        .collect();
                    Ok(format!(
                        "[\n{}{}\n{}]",
                        next_indent,
                        items?.join(&format!(",\n{}", next_indent)),
                        indent
                    ))
                } else {
                    Err("Tuple should have been accessible".to_string())
                }
            }
            HeapTag::LBox => Err("LBox should have been handled above".to_string()),
            HeapTag::CaptureCell => Err("Cannot serialize capture cell to JSON".to_string()),
            HeapTag::Float => {
                // This is a heap-allocated float (for NaN values)
                Err("Cannot serialize non-finite float value to JSON".to_string())
            }
            HeapTag::LibHandle => Err("Cannot serialize library handles to JSON".to_string()),

            HeapTag::ThreadHandle => Err("Cannot serialize thread handles to JSON".to_string()),
            HeapTag::Fiber => Err("Cannot serialize fibers to JSON".to_string()),
            HeapTag::Syntax => Err("Cannot serialize syntax objects to JSON".to_string()),
            HeapTag::FFISignature => Err("Cannot serialize FFI signatures to JSON".to_string()),
            HeapTag::FFIType => Err("Cannot serialize FFI type descriptors to JSON".to_string()),
            HeapTag::ManagedPointer => Err("Cannot serialize pointers to JSON".to_string()),
            HeapTag::LStringMut => Err("Cannot serialize buffers to JSON".to_string()),
            HeapTag::LBytes => Err("Cannot serialize bytes to JSON".to_string()),
            HeapTag::LBytesMut => Err("Cannot serialize blobs to JSON".to_string()),
            HeapTag::External => Err("Cannot serialize external objects to JSON".to_string()),
            HeapTag::Parameter => Err("Cannot serialize parameters to JSON".to_string()),
            HeapTag::LSet => {
                if let Some(s) = value.as_set() {
                    let items: Result<Vec<String>, String> = s
                        .iter()
                        .map(|v| serialize_value_pretty(v, symbols, indent_level + 1))
                        .collect();
                    Ok(format!(
                        "[\n{}{}\n{}]",
                        next_indent,
                        items?.join(&format!(",\n{}", next_indent)),
                        indent
                    ))
                } else {
                    Err("Set should have been accessible".to_string())
                }
            }
            HeapTag::LSetMut => {
                if let Some(s_ref) = value.as_set_mut() {
                    let s = s_ref.borrow();
                    let items: Result<Vec<String>, String> = s
                        .iter()
                        .map(|v| serialize_value_pretty(v, symbols, indent_level + 1))
                        .collect();
                    Ok(format!(
                        "[\n{}{}\n{}]",
                        next_indent,
                        items?.join(&format!(",\n{}", next_indent)),
                        indent
                    ))
                } else {
                    Err("Mutable set should have been accessible".to_string())
                }
            }
            HeapTag::ClosureTemplate => {
                Err("Cannot serialize closure templates to JSON".to_string())
            }
        }
    } else {
        Err("Cannot serialize unknown value type to JSON".to_string())
    }
}
