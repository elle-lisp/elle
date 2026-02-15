//! Vector operations primitives
use crate::error::{LError, LResult};
use crate::value::Value;
use std::rc::Rc;

/// Create a vector from arguments
pub fn prim_vector(args: &[Value]) -> LResult<Value> {
    Ok(Value::Vector(Rc::new(args.to_vec())))
}

/// Get the length of a vector
pub fn prim_vector_length(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }

    match &args[0] {
        Value::Vector(v) => Ok(Value::Int(v.len() as i64)),
        _ => Err(LError::type_mismatch("vector", args[0].type_name())),
    }
}

/// Get a reference from a vector at an index
pub fn prim_vector_ref(args: &[Value]) -> LResult<Value> {
    if args.len() != 2 {
        return Err(LError::arity_mismatch(2, args.len()));
    }

    let vec = args[0].as_vector()?;
    let index = args[1].as_int()? as usize;

    vec.get(index)
        .cloned()
        .ok_or_else(|| LError::index_out_of_bounds(index as isize, vec.len()))
}

/// Set a value in a vector at an index (returns new vector)
pub fn prim_vector_set(args: &[Value]) -> LResult<Value> {
    if args.len() != 3 {
        return Err(LError::arity_mismatch(3, args.len()));
    }

    let mut vec = args[0].as_vector()?.as_ref().clone();
    let index = args[1].as_int()? as usize;
    let value = args[2].clone();

    if index >= vec.len() {
        return Err(LError::index_out_of_bounds(index as isize, vec.len()));
    }

    vec[index] = value;
    Ok(Value::Vector(Rc::new(vec)))
}
