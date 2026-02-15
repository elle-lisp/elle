//! List manipulation primitives
use crate::error::{LError, LResult};
use crate::symbol::SymbolTable;
use crate::value::{list, SymbolId, Value};
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
pub fn prim_cons(args: &[Value]) -> LResult<Value> {
    if args.len() != 2 {
        return Err(LError::arity_mismatch(2, args.len()));
    }
    Ok(crate::value::cons(args[0].clone(), args[1].clone()))
}

/// Get the first element of a cons cell
pub fn prim_first(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }
    let cons = args[0].as_cons()?;
    Ok(cons.first.clone())
}

/// Get the rest of a cons cell
pub fn prim_rest(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }
    let cons = args[0].as_cons()?;
    Ok(cons.rest.clone())
}

/// Create a list from arguments
pub fn prim_list(args: &[Value]) -> LResult<Value> {
    Ok(list(args.to_vec()))
}

/// Get the length of a collection (universal for all container types)
pub fn prim_length(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }

    match &args[0] {
        // For lists: convert to vec and get length
        Value::Nil => Ok(Value::Int(0)),
        Value::Cons(_) => {
            let vec = args[0].list_to_vec()?;
            Ok(Value::Int(vec.len() as i64))
        }

        // For strings: get character count
        Value::String(s) => Ok(Value::Int(s.chars().count() as i64)),

        // For vectors: get length (Rc<Vec<Value>>)
        Value::Vector(v) => Ok(Value::Int(v.len() as i64)),

        // For tables (hash maps): get entry count (Rc<RefCell<BTreeMap>>)
        Value::Table(t) => Ok(Value::Int(t.borrow().len() as i64)),

        // For structs: get field count (Rc<BTreeMap>)
        Value::Struct(s) => Ok(Value::Int(s.len() as i64)),

        // For symbols: get the length of the symbol name
        Value::Symbol(sid) => {
            // Get the symbol name from the symbol table context
            if let Some(name) = get_keyword_name(*sid) {
                Ok(Value::Int(name.chars().count() as i64))
            } else {
                Err(LError::runtime_error(format!(
                    "Unable to resolve symbol name for id {:?}",
                    sid
                )))
            }
        }

        // For keywords: get the length of the keyword name
        Value::Keyword(kid) => {
            // Get the keyword name from the symbol table context
            if let Some(name) = get_keyword_name(*kid) {
                Ok(Value::Int(name.chars().count() as i64))
            } else {
                Err(LError::runtime_error(format!(
                    "Unable to resolve keyword name for id {:?}",
                    kid
                )))
            }
        }

        // Other types are not sequences
        _ => Err(LError::type_mismatch(
            "collection (list, string, vector, table, struct, symbol, or keyword)",
            args[0].type_name(),
        )),
    }
}

/// Check if a collection is empty (O(1) operation for most types)
pub fn prim_empty(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }

    match &args[0] {
        // For lists: just check if it's nil
        Value::Nil => Ok(Value::Bool(true)),
        Value::Cons(_) => Ok(Value::Bool(false)),

        // For strings: check if empty
        Value::String(s) => Ok(Value::Bool(s.is_empty())),

        // For vectors: check length (Rc<Vec<Value>>)
        Value::Vector(v) => Ok(Value::Bool(v.is_empty())),

        // For tables (hash maps): check if empty (Rc<RefCell<BTreeMap>>)
        Value::Table(t) => Ok(Value::Bool(t.borrow().is_empty())),

        // For structs: check field count (Rc<BTreeMap>)
        Value::Struct(s) => Ok(Value::Bool(s.is_empty())),

        // Other types are not sequences
        _ => Err(LError::type_mismatch(
            "collection (list, string, vector, table, or struct)",
            args[0].type_name(),
        )),
    }
}

/// Append multiple lists
pub fn prim_append(args: &[Value]) -> LResult<Value> {
    let mut result = Vec::new();
    for arg in args {
        let vec = arg.list_to_vec()?;
        result.extend(vec);
    }
    Ok(list(result))
}

/// Reverse a list
pub fn prim_reverse(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }
    let mut vec = args[0].list_to_vec()?;
    vec.reverse();
    Ok(list(vec))
}

/// Get the nth element of a list
pub fn prim_nth(args: &[Value]) -> LResult<Value> {
    if args.len() != 2 {
        return Err(LError::arity_mismatch(2, args.len()));
    }

    let index = args[0].as_int()? as usize;
    let vec = args[1].list_to_vec()?;

    vec.get(index)
        .cloned()
        .ok_or_else(|| LError::index_out_of_bounds(index as isize, vec.len()))
}

/// Get the last element of a list
pub fn prim_last(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }

    let vec = args[0].list_to_vec()?;
    vec.last()
        .cloned()
        .ok_or_else(|| LError::runtime_error("Cannot get last of empty list"))
}

/// Take the first n elements of a list
pub fn prim_take(args: &[Value]) -> LResult<Value> {
    if args.len() != 2 {
        return Err(LError::arity_mismatch(2, args.len()));
    }

    let count = args[0].as_int()? as usize;
    let vec = args[1].list_to_vec()?;

    let taken: Vec<Value> = vec.into_iter().take(count).collect();
    Ok(list(taken))
}

/// Drop the first n elements of a list
pub fn prim_drop(args: &[Value]) -> LResult<Value> {
    if args.len() != 2 {
        return Err(LError::arity_mismatch(2, args.len()));
    }

    let count = args[0].as_int()? as usize;
    let vec = args[1].list_to_vec()?;

    let dropped: Vec<Value> = vec.into_iter().skip(count).collect();
    Ok(list(dropped))
}
