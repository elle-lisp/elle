//! String manipulation primitives
use crate::error::{LError, LResult};
use crate::value::{Condition, Value};
use crate::vm::VM;

/// Get the length of a string
pub fn prim_string_length(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "string-length: expected 1 argument, got {}",
            args.len()
        )));
    }
    match args[0].as_string() {
        Some(s) => Ok(Value::int(s.chars().count() as i64)),
        None => Err(Condition::type_error(format!(
            "string-length: expected string, got {}",
            args[0].type_name()
        ))),
    }
}

/// Append multiple strings
pub fn prim_string_append(args: &[Value]) -> Result<Value, Condition> {
    let mut result = String::new();
    for arg in args {
        match arg.as_string() {
            Some(s) => result.push_str(s),
            None => {
                return Err(Condition::type_error(format!(
                    "string-append: expected string, got {}",
                    arg.type_name()
                )))
            }
        }
    }
    Ok(Value::string(result))
}

/// Convert string to uppercase
pub fn prim_string_upcase(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "string-upcase: expected 1 argument, got {}",
            args.len()
        )));
    }
    match args[0].as_string() {
        Some(s) => Ok(Value::string(s.to_uppercase())),
        None => Err(Condition::type_error(format!(
            "string-upcase: expected string, got {}",
            args[0].type_name()
        ))),
    }
}

/// Convert string to lowercase
pub fn prim_string_downcase(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "string-downcase: expected 1 argument, got {}",
            args.len()
        )));
    }
    match args[0].as_string() {
        Some(s) => Ok(Value::string(s.to_lowercase())),
        None => Err(Condition::type_error(format!(
            "string-downcase: expected string, got {}",
            args[0].type_name()
        ))),
    }
}

/// Get a substring
pub fn prim_substring(args: &[Value]) -> Result<Value, Condition> {
    if args.len() < 2 || args.len() > 3 {
        return Err(Condition::arity_error(format!(
            "substring: expected 2-3 arguments, got {}",
            args.len()
        )));
    }

    let s = match args[0].as_string() {
        Some(s) => s,
        None => {
            return Err(Condition::type_error(format!(
                "substring: expected string, got {}",
                args[0].type_name()
            )))
        }
    };

    let start = match args[1].as_int() {
        Some(n) => n as usize,
        None => {
            return Err(Condition::type_error(format!(
                "substring: expected integer, got {}",
                args[1].type_name()
            )))
        }
    };
    let char_count = s.chars().count();
    let end = if args.len() == 3 {
        match args[2].as_int() {
            Some(n) => n as usize,
            None => {
                return Err(Condition::type_error(format!(
                    "substring: expected integer, got {}",
                    args[2].type_name()
                )))
            }
        }
    } else {
        char_count
    };

    if start > char_count || end > char_count || start > end {
        return Err(Condition::error(format!(
            "substring: index {} out of bounds (length {})",
            start, char_count
        )));
    }

    // Convert character indices to byte indices
    let byte_start = s
        .char_indices()
        .nth(start)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    let byte_end = s.char_indices().nth(end).map(|(i, _)| i).unwrap_or(s.len());
    Ok(Value::string(&s[byte_start..byte_end]))
}

/// Find the index of a character
pub fn prim_string_index(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "string-index: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let haystack = match args[0].as_string() {
        Some(s) => s,
        None => {
            return Err(Condition::type_error(format!(
                "string-index: expected string, got {}",
                args[0].type_name()
            )))
        }
    };

    let needle = match args[1].as_string() {
        Some(s) => {
            if s.chars().count() != 1 {
                return Err(Condition::error(
                    "string-index: requires a single character as second argument".to_string(),
                ));
            }
            s.chars().next().unwrap()
        }
        None => {
            return Err(Condition::type_error(format!(
                "string-index: expected string, got {}",
                args[1].type_name()
            )))
        }
    };

    match haystack.chars().position(|ch| ch == needle) {
        Some(pos) => Ok(Value::int(pos as i64)),
        None => Ok(Value::NIL),
    }
}

/// Get a character at an index
pub fn prim_char_at(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "char-at: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let s = match args[0].as_string() {
        Some(s) => s,
        None => {
            return Err(Condition::type_error(format!(
                "char-at: expected string, got {}",
                args[0].type_name()
            )))
        }
    };

    let index = match args[1].as_int() {
        Some(n) => n as usize,
        None => {
            return Err(Condition::type_error(format!(
                "char-at: expected integer, got {}",
                args[1].type_name()
            )))
        }
    };
    let char_count = s.chars().count();

    if index >= char_count {
        return Err(Condition::error(format!(
            "char-at: index {} out of bounds (length {})",
            index, char_count
        )));
    }

    match s.chars().nth(index) {
        Some(c) => Ok(Value::string(c.to_string())),
        None => Err(Condition::error(format!(
            "char-at: index {} out of bounds (length {})",
            index, char_count
        ))),
    }
}

/// Convert to integer
pub fn prim_to_int(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "to-int: expected 1 argument, got {}",
            args.len()
        )));
    }
    match args[0].as_int() {
        Some(n) => Ok(Value::int(n)),
        None => match args[0].as_float() {
            Some(f) => Ok(Value::int(f as i64)),
            None => match args[0].as_string() {
                Some(s) => s.parse::<i64>().map(Value::int).map_err(|_| {
                    Condition::error("to-int: cannot parse string as integer".to_string())
                }),
                None => Err(Condition::type_error(format!(
                    "to-int: expected integer, float, or string, got {}",
                    args[0].type_name()
                ))),
            },
        },
    }
}

/// Convert to float
pub fn prim_to_float(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "to-float: expected 1 argument, got {}",
            args.len()
        )));
    }
    match args[0].as_int() {
        Some(n) => Ok(Value::float(n as f64)),
        None => match args[0].as_float() {
            Some(f) => Ok(Value::float(f)),
            None => match args[0].as_string() {
                Some(s) => s.parse::<f64>().map(Value::float).map_err(|_| {
                    Condition::error("to-float: cannot parse string as float".to_string())
                }),
                None => Err(Condition::type_error(format!(
                    "to-float: expected integer, float, or string, got {}",
                    args[0].type_name()
                ))),
            },
        },
    }
}

/// Convert to string
pub fn prim_to_string(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "to-string: expected 1 argument, got {}",
            args.len()
        )));
    }

    let val = args[0];

    // Handle immediate types
    if let Some(s) = val.as_string() {
        return Ok(Value::string(s));
    }

    if let Some(n) = val.as_int() {
        return Ok(Value::string(n.to_string()));
    }

    if let Some(f) = val.as_float() {
        return Ok(Value::string(f.to_string()));
    }

    if let Some(b) = val.as_bool() {
        return Ok(Value::string(if b { "true" } else { "false" }));
    }

    if val.is_nil() {
        return Ok(Value::string("nil"));
    }

    if let Some(sym_id) = val.as_symbol() {
        // Get symbol name from symbol table
        unsafe {
            if let Some(symbols_ptr) = crate::ffi::primitives::context::get_symbol_table() {
                let symbols = &*symbols_ptr;
                let sym_id = crate::value::SymbolId(sym_id);
                if let Some(name) = symbols.name(sym_id) {
                    return Ok(Value::string(name));
                } else {
                    return Err(Condition::error(format!(
                        "to-string: symbol ID {} not found in symbol table",
                        sym_id.0
                    )));
                }
            } else {
                return Err(Condition::error(
                    "to-string: symbol table not available".to_string(),
                ));
            }
        }
    }

    if let Some(kw_id) = val.as_keyword() {
        // Get keyword name from symbol table with colon prefix
        unsafe {
            if let Some(symbols_ptr) = crate::ffi::primitives::context::get_symbol_table() {
                let symbols = &*symbols_ptr;
                let sym_id = crate::value::SymbolId(kw_id);
                if let Some(name) = symbols.name(sym_id) {
                    return Ok(Value::string(format!(":{}", name)));
                } else {
                    return Err(Condition::error(format!(
                        "to-string: keyword ID {} not found in symbol table",
                        sym_id.0
                    )));
                }
            } else {
                return Err(Condition::error(
                    "to-string: symbol table not available".to_string(),
                ));
            }
        }
    }

    // Handle heap types (Cons, Vector, etc.)
    if let Some(_cons) = val.as_cons() {
        // Format as list "(1 2 3)"
        let mut items = Vec::new();
        let mut current = val;
        loop {
            if current.is_nil() {
                break;
            }
            if let Some(c) = current.as_cons() {
                items.push(c.first);
                current = c.rest;
            } else {
                // Improper list - add the tail
                items.push(current);
                break;
            }
        }

        let formatted_items: Result<Vec<String>, _> = items
            .iter()
            .map(|v| {
                prim_to_string(&[*v]).and_then(|s| {
                    s.as_string().map(|s| s.to_string()).ok_or_else(|| {
                        Condition::error("to-string: failed to convert list item".to_string())
                    })
                })
            })
            .collect();

        match formatted_items {
            Ok(strs) => {
                let list_str = format!("({})", strs.join(" "));
                return Ok(Value::string(list_str));
            }
            Err(e) => return Err(e),
        }
    }

    if let Some(vec_ref) = val.as_vector() {
        // Format as "[1, 2, 3]"
        let vec = vec_ref.borrow();
        let formatted_items: Result<Vec<String>, _> = vec
            .iter()
            .map(|v| {
                prim_to_string(&[*v]).and_then(|s| {
                    s.as_string().map(|s| s.to_string()).ok_or_else(|| {
                        Condition::error("to-string: failed to convert vector item".to_string())
                    })
                })
            })
            .collect();

        match formatted_items {
            Ok(strs) => {
                let vec_str = format!("[{}]", strs.join(", "));
                return Ok(Value::string(vec_str));
            }
            Err(e) => return Err(e),
        }
    }

    // For other types, use a reasonable debug representation
    Ok(Value::string(format!("{:?}", val)))
}

/// Split string on delimiter
pub fn prim_string_split(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "string-split: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let s = match args[0].as_string() {
        Some(s) => s,
        None => {
            return Err(Condition::type_error(format!(
                "string-split: expected string, got {}",
                args[0].type_name()
            )))
        }
    };

    let delimiter = match args[1].as_string() {
        Some(d) => d,
        None => {
            return Err(Condition::type_error(format!(
                "string-split: expected string, got {}",
                args[1].type_name()
            )))
        }
    };

    if delimiter.is_empty() {
        return Err(Condition::error(
            "string-split: delimiter cannot be empty".to_string(),
        ));
    }

    let parts: Vec<Value> = s.split(delimiter).map(Value::string).collect();

    Ok(crate::value::list(parts))
}

/// Replace all occurrences of old with new
pub fn prim_string_replace(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 3 {
        return Err(Condition::arity_error(format!(
            "string-replace: expected 3 arguments, got {}",
            args.len()
        )));
    }

    let s = match args[0].as_string() {
        Some(s) => s,
        None => {
            return Err(Condition::type_error(format!(
                "string-replace: expected string, got {}",
                args[0].type_name()
            )))
        }
    };

    let old = match args[1].as_string() {
        Some(o) => o,
        None => {
            return Err(Condition::type_error(format!(
                "string-replace: expected string, got {}",
                args[1].type_name()
            )))
        }
    };

    if old.is_empty() {
        return Err(Condition::error(
            "string-replace: search string cannot be empty".to_string(),
        ));
    }

    let new = match args[2].as_string() {
        Some(n) => n,
        None => {
            return Err(Condition::type_error(format!(
                "string-replace: expected string, got {}",
                args[2].type_name()
            )))
        }
    };

    Ok(Value::string(s.replace(old, new)))
}

/// Trim leading and trailing whitespace
pub fn prim_string_trim(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "string-trim: expected 1 argument, got {}",
            args.len()
        )));
    }

    match args[0].as_string() {
        Some(s) => Ok(Value::string(s.trim())),
        None => Err(Condition::type_error(format!(
            "string-trim: expected string, got {}",
            args[0].type_name()
        ))),
    }
}

/// Check if string contains substring
pub fn prim_string_contains(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "string-contains?: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let haystack = match args[0].as_string() {
        Some(s) => s,
        None => {
            return Err(Condition::type_error(format!(
                "string-contains?: expected string, got {}",
                args[0].type_name()
            )))
        }
    };

    let needle = match args[1].as_string() {
        Some(n) => n,
        None => {
            return Err(Condition::type_error(format!(
                "string-contains?: expected string, got {}",
                args[1].type_name()
            )))
        }
    };

    Ok(if haystack.contains(needle) {
        Value::TRUE
    } else {
        Value::FALSE
    })
}

/// Check if string starts with prefix
pub fn prim_string_starts_with(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "string-starts-with?: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let s = match args[0].as_string() {
        Some(s) => s,
        None => {
            return Err(Condition::type_error(format!(
                "string-starts-with?: expected string, got {}",
                args[0].type_name()
            )))
        }
    };

    let prefix = match args[1].as_string() {
        Some(p) => p,
        None => {
            return Err(Condition::type_error(format!(
                "string-starts-with?: expected string, got {}",
                args[1].type_name()
            )))
        }
    };

    Ok(if s.starts_with(prefix) {
        Value::TRUE
    } else {
        Value::FALSE
    })
}

/// Check if string ends with suffix
pub fn prim_string_ends_with(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "string-ends-with?: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let s = match args[0].as_string() {
        Some(s) => s,
        None => {
            return Err(Condition::type_error(format!(
                "string-ends-with?: expected string, got {}",
                args[0].type_name()
            )))
        }
    };

    let suffix = match args[1].as_string() {
        Some(suf) => suf,
        None => {
            return Err(Condition::type_error(format!(
                "string-ends-with?: expected string, got {}",
                args[1].type_name()
            )))
        }
    };

    Ok(if s.ends_with(suffix) {
        Value::TRUE
    } else {
        Value::FALSE
    })
}

/// Join list of strings with separator
pub fn prim_string_join(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "string-join: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let list = &args[0];
    let separator = match args[1].as_string() {
        Some(s) => s,
        None => {
            return Err(Condition::type_error(format!(
                "string-join: expected string, got {}",
                args[1].type_name()
            )))
        }
    };

    let vec = list
        .list_to_vec()
        .map_err(|e| Condition::type_error(format!("string-join: {}", e)))?;
    let mut strings = Vec::new();

    for val in vec {
        match val.as_string() {
            Some(s) => strings.push(s.to_string()),
            None => {
                return Err(Condition::type_error(format!(
                    "string-join: expected string, got {}",
                    val.type_name()
                )))
            }
        }
    }

    Ok(Value::string(strings.join(separator)))
}

/// Convert number to string
pub fn prim_number_to_string(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "number->string: expected 1 argument, got {}",
            args.len()
        )));
    }

    match args[0].as_int() {
        Some(n) => Ok(Value::string(n.to_string())),
        None => match args[0].as_float() {
            Some(f) => Ok(Value::string(f.to_string())),
            None => Err(Condition::type_error(format!(
                "number->string: expected number, got {}",
                args[0].type_name()
            ))),
        },
    }
}

// ============ SCHEME-STYLE CONVERSION ALIASES ============

/// Convert string to integer (Scheme-style name)
/// `(string->int str)`
pub fn prim_string_to_int(args: &[Value]) -> Result<Value, Condition> {
    prim_to_int(args)
}

/// Convert string to float (Scheme-style name)
/// `(string->float str)`
pub fn prim_string_to_float(args: &[Value]) -> Result<Value, Condition> {
    prim_to_float(args)
}

/// Convert any value to string (Scheme-style name)
/// `(any->string val)`
pub fn prim_any_to_string(args: &[Value]) -> Result<Value, Condition> {
    prim_to_string(args)
}

/// Convert symbol to string
/// `(symbol->string sym)`
pub fn prim_symbol_to_string(args: &[Value], vm: &mut VM) -> LResult<Value> {
    if args.len() != 1 {
        let cond = Condition::arity_error(format!(
            "symbol->string: expected 1 argument, got {}",
            args.len()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        return Ok(Value::NIL);
    }

    match args[0].as_symbol() {
        Some(id) => {
            // SAFETY: The symbol table is set in main.rs before any code execution
            unsafe {
                if let Some(symbols_ptr) = crate::ffi::primitives::context::get_symbol_table() {
                    let symbols = &*symbols_ptr;
                    let sym_id = crate::value::SymbolId(id);
                    if let Some(name) = symbols.name(sym_id) {
                        Ok(Value::string(name))
                    } else {
                        // Symbol ID not found is a VM bug - the symbol table should be consistent
                        Err(LError::generic(format!(
                            "Symbol ID {} not found in symbol table",
                            id
                        )))
                    }
                } else {
                    // Symbol table not available is a VM bug - it should always be set
                    Err(LError::generic("Symbol table not available"))
                }
            }
        }
        None => {
            let cond = Condition::type_error(format!(
                "symbol->string: expected symbol, got {}",
                args[0].type_name()
            ));
            vm.current_exception = Some(std::rc::Rc::new(cond));
            Ok(Value::NIL)
        }
    }
}
