//! List manipulation primitives
use crate::symbol::SymbolTable;
use crate::value::{list, Condition, SymbolId, Value};
use std::cell::RefCell;

thread_local! {
    static SYMBOL_TABLE: RefCell<Option<*mut SymbolTable>> = const { RefCell::new(None) };
}

/// Set the symbol table context for length primitive
///
/// # Safety
/// The pointer must remain valid for the duration of use.
pub fn set_length_symbol_table(symbols: *mut SymbolTable) {
    SYMBOL_TABLE.with(|st| {
        *st.borrow_mut() = Some(symbols);
    });
}

/// Clear the symbol table context
pub fn clear_length_symbol_table() {
    SYMBOL_TABLE.with(|st| {
        *st.borrow_mut() = None;
    });
}

/// Get the keyword name from a keyword ID
fn get_keyword_name(kid: SymbolId) -> Option<String> {
    SYMBOL_TABLE.with(|st| {
        let ptr = st.borrow();
        match *ptr {
            Some(p) => {
                // SAFETY: Caller ensures pointer validity via set_length_symbol_table
                let symbols = unsafe { &*p };
                symbols.name(kid).map(|s| s.to_string())
            }
            None => None,
        }
    })
}

/// Construct a cons cell
pub fn prim_cons(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "cons: expected 2 arguments, got {}",
            args.len()
        )));
    }
    Ok(crate::value::cons(args[0], args[1]))
}

/// Get the first element of a cons cell
pub fn prim_first(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "first: expected 1 argument, got {}",
            args.len()
        )));
    }
    let cons = args[0].as_cons().ok_or_else(|| {
        Condition::type_error(format!(
            "first: expected cons cell, got {}",
            args[0].type_name()
        ))
    })?;
    Ok(cons.first)
}

/// Get the rest of a cons cell
pub fn prim_rest(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "rest: expected 1 argument, got {}",
            args.len()
        )));
    }
    let cons = args[0].as_cons().ok_or_else(|| {
        Condition::type_error(format!(
            "rest: expected cons cell, got {}",
            args[0].type_name()
        ))
    })?;
    Ok(cons.rest)
}

/// Create a list from arguments
pub fn prim_list(args: &[Value]) -> Result<Value, Condition> {
    Ok(list(args.to_vec()))
}

/// Get the length of a collection (universal for all container types)
pub fn prim_length(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "length: expected 1 argument, got {}",
            args.len()
        )));
    }

    if args[0].is_nil() || args[0].is_empty_list() {
        Ok(Value::int(0))
    } else if args[0].is_cons() {
        let vec = args[0]
            .list_to_vec()
            .map_err(|e| Condition::type_error(format!("length: {}", e)))?;
        Ok(Value::int(vec.len() as i64))
    } else if let Some(s) = args[0].as_string() {
        Ok(Value::int(s.chars().count() as i64))
    } else if args[0].is_vector() {
        let vec = args[0]
            .as_vector()
            .ok_or_else(|| Condition::error("length: failed to get vector".to_string()))?;
        Ok(Value::int(vec.borrow().len() as i64))
    } else if args[0].is_table() {
        let table = args[0]
            .as_table()
            .ok_or_else(|| Condition::error("length: failed to get table".to_string()))?;
        Ok(Value::int(table.borrow().len() as i64))
    } else if args[0].is_struct() {
        let s = args[0]
            .as_struct()
            .ok_or_else(|| Condition::error("length: failed to get struct".to_string()))?;
        Ok(Value::int(s.len() as i64))
    } else if let Some(sid) = args[0].as_symbol() {
        // Get the symbol name from the symbol table context
        if let Some(name) = get_keyword_name(crate::value_old::SymbolId(sid)) {
            Ok(Value::int(name.chars().count() as i64))
        } else {
            Err(Condition::error(format!(
                "length: unable to resolve symbol name for id {:?}",
                sid
            )))
        }
    } else if let Some(kid) = args[0].as_keyword() {
        // Get the keyword name from the symbol table context
        if let Some(name) = get_keyword_name(crate::value_old::SymbolId(kid)) {
            Ok(Value::int(name.chars().count() as i64))
        } else {
            Err(Condition::error(format!(
                "length: unable to resolve keyword name for id {:?}",
                kid
            )))
        }
    } else {
        Err(Condition::type_error(format!(
            "length: expected collection type (list, string, vector, table, struct, symbol, or keyword), got {}",
            args[0].type_name()
        )))
    }
}

/// Check if a collection is empty (O(1) operation for most types)
pub fn prim_empty(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "empty?: expected 1 argument, got {}",
            args.len()
        )));
    }

    // nil is not a container - error if passed
    if args[0].is_nil() {
        return Err(Condition::type_error(
            "empty?: expected collection type (list, string, vector, table, or struct), got nil"
                .to_string(),
        ));
    }

    let result = if args[0].is_empty_list() {
        true
    } else if args[0].is_cons() {
        false
    } else if let Some(s) = args[0].as_string() {
        s.is_empty()
    } else if args[0].is_vector() {
        let vec = args[0]
            .as_vector()
            .ok_or_else(|| Condition::error("empty?: failed to get vector".to_string()))?;
        vec.borrow().is_empty()
    } else if args[0].is_table() {
        let table = args[0]
            .as_table()
            .ok_or_else(|| Condition::error("empty?: failed to get table".to_string()))?;
        table.borrow().is_empty()
    } else if args[0].is_struct() {
        let s = args[0]
            .as_struct()
            .ok_or_else(|| Condition::error("empty?: failed to get struct".to_string()))?;
        s.is_empty()
    } else {
        return Err(Condition::type_error(format!(
            "empty?: expected collection type (list, string, vector, table, or struct), got {}",
            args[0].type_name()
        )));
    };

    Ok(if result { Value::TRUE } else { Value::FALSE })
}

/// Append multiple lists
pub fn prim_append(args: &[Value]) -> Result<Value, Condition> {
    let mut result = Vec::new();
    for arg in args {
        let vec = arg
            .list_to_vec()
            .map_err(|e| Condition::type_error(format!("append: {}", e)))?;
        result.extend(vec);
    }
    Ok(list(result))
}

/// Reverse a list
pub fn prim_reverse(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "reverse: expected 1 argument, got {}",
            args.len()
        )));
    }
    let mut vec = args[0]
        .list_to_vec()
        .map_err(|e| Condition::type_error(format!("reverse: {}", e)))?;
    vec.reverse();
    Ok(list(vec))
}

/// Get the nth element of a list
pub fn prim_nth(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "nth: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let index = match args[0].as_int() {
        Some(n) => n as usize,
        None => {
            return Err(Condition::type_error(format!(
                "nth: expected integer, got {}",
                args[0].type_name()
            )))
        }
    };
    let vec = args[1]
        .list_to_vec()
        .map_err(|e| Condition::type_error(format!("nth: {}", e)))?;

    vec.get(index).cloned().ok_or_else(|| {
        Condition::error(format!(
            "nth: index {} out of bounds (length {})",
            index,
            vec.len()
        ))
    })
}

/// Get the last element of a list
pub fn prim_last(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "last: expected 1 argument, got {}",
            args.len()
        )));
    }

    let vec = args[0]
        .list_to_vec()
        .map_err(|e| Condition::type_error(format!("last: {}", e)))?;
    vec.last()
        .cloned()
        .ok_or_else(|| Condition::error("last: cannot get last of empty list".to_string()))
}

/// Take the first n elements of a list
pub fn prim_take(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "take: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let count = match args[0].as_int() {
        Some(n) => n as usize,
        None => {
            return Err(Condition::type_error(format!(
                "take: expected integer, got {}",
                args[0].type_name()
            )))
        }
    };
    let vec = args[1]
        .list_to_vec()
        .map_err(|e| Condition::type_error(format!("take: {}", e)))?;

    let taken: Vec<Value> = vec.into_iter().take(count).collect();
    Ok(list(taken))
}

/// Drop the first n elements of a list
pub fn prim_drop(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "drop: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let count = match args[0].as_int() {
        Some(n) => n as usize,
        None => {
            return Err(Condition::type_error(format!(
                "drop: expected integer, got {}",
                args[0].type_name()
            )))
        }
    };
    let vec = args[1]
        .list_to_vec()
        .map_err(|e| Condition::type_error(format!("drop: {}", e)))?;

    let dropped: Vec<Value> = vec.into_iter().skip(count).collect();
    Ok(list(dropped))
}
