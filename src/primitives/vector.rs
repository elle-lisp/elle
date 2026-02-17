//! Vector operations primitives
use crate::value::{Condition, Value};

/// Create a vector from arguments
pub fn prim_vector(args: &[Value]) -> Result<Value, Condition> {
    Ok(Value::vector(args.to_vec()))
}

/// Get the length of a vector
pub fn prim_vector_length(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "vector-length: expected 1 argument, got {}",
            args.len()
        )));
    }

    if let Some(v) = args[0].as_vector() {
        let borrowed = v.borrow();
        Ok(Value::int(borrowed.len() as i64))
    } else {
        Err(Condition::type_error(format!(
            "vector-length: expected vector, got {}",
            args[0].type_name()
        )))
    }
}

/// Get a reference from a vector at an index
pub fn prim_vector_ref(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "vector-ref: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let vec = args[0].as_vector().ok_or_else(|| {
        Condition::type_error(format!(
            "vector-ref: expected vector, got {}",
            args[0].type_name()
        ))
    })?;
    let index = args[1].as_int().ok_or_else(|| {
        Condition::type_error(format!(
            "vector-ref: expected integer, got {}",
            args[1].type_name()
        ))
    })? as usize;

    let borrowed = vec.borrow();
    borrowed.get(index).cloned().ok_or_else(|| {
        Condition::error(format!(
            "vector-ref: index {} out of bounds (length {})",
            index,
            borrowed.len()
        ))
    })
}

/// Set a value in a vector at an index (returns new vector)
pub fn prim_vector_set(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 3 {
        return Err(Condition::arity_error(format!(
            "vector-set!: expected 3 arguments, got {}",
            args.len()
        )));
    }

    let vec_ref = args[0].as_vector().ok_or_else(|| {
        Condition::type_error(format!(
            "vector-set!: expected vector, got {}",
            args[0].type_name()
        ))
    })?;
    let index = args[1].as_int().ok_or_else(|| {
        Condition::type_error(format!(
            "vector-set!: expected integer, got {}",
            args[1].type_name()
        ))
    })? as usize;
    let value = args[2];

    let mut vec = vec_ref.borrow_mut();
    if index >= vec.len() {
        return Err(Condition::error(format!(
            "vector-set!: index {} out of bounds (length {})",
            index,
            vec.len()
        )));
    }

    vec[index] = value;
    drop(vec);
    Ok(Value::vector(vec_ref.borrow().clone()))
}
