//! Higher-order function primitives (map, filter, fold)
use crate::value::{list, Condition, Value};

/// Apply a function to each element of a list
pub fn prim_map(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(
            "map: expected 2 arguments, got ".to_string() + &args.len().to_string(),
        ));
    }

    if let Some(f) = args[0].as_native_fn() {
        let vec = args[1]
            .list_to_vec()
            .map_err(|e| Condition::type_error(format!("map: {}", e)))?;
        let results: Result<Vec<Value>, Condition> =
            vec.iter().map(|v| f(std::slice::from_ref(v))).collect();
        Ok(list(results?))
    } else if args[0].is_closure() {
        Err(Condition::error(
            "map with closures not yet supported (use native functions or ffi_map)".to_string(),
        ))
    } else {
        Err(Condition::type_error(
            "map: first argument must be a function".to_string(),
        ))
    }
}

/// Filter a list using a predicate function
pub fn prim_filter(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(
            "filter: expected 2 arguments, got ".to_string() + &args.len().to_string(),
        ));
    }

    if let Some(f) = args[0].as_native_fn() {
        let vec = args[1]
            .list_to_vec()
            .map_err(|e| Condition::type_error(format!("filter: {}", e)))?;
        let mut results = Vec::new();
        for v in vec {
            let result = f(std::slice::from_ref(&v))?;
            if !result.is_nil() && result != Value::FALSE {
                results.push(v);
            }
        }
        Ok(list(results))
    } else if args[0].is_closure() {
        Err(Condition::error(
            "filter with closures not yet supported (use native functions or ffi_filter)"
                .to_string(),
        ))
    } else {
        Err(Condition::type_error(
            "filter: first argument must be a predicate function".to_string(),
        ))
    }
}

/// Fold (reduce) a list with an accumulator
pub fn prim_fold(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 3 {
        return Err(Condition::arity_error(
            "fold: expected 3 arguments, got ".to_string() + &args.len().to_string(),
        ));
    }

    if let Some(f) = args[0].as_native_fn() {
        let mut accumulator = args[1];
        let vec = args[2]
            .list_to_vec()
            .map_err(|e| Condition::type_error(format!("fold: {}", e)))?;
        for v in vec {
            accumulator = f(&[accumulator, v])?;
        }
        Ok(accumulator)
    } else if args[0].is_closure() {
        Err(Condition::error(
            "fold with closures not yet supported (use native functions or ffi_fold)".to_string(),
        ))
    } else {
        Err(Condition::type_error(
            "fold: first argument must be a function".to_string(),
        ))
    }
}
