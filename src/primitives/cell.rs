//! Cell/Box primitives for mutable storage
use crate::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

/// Create a mutable cell containing a value
///
/// (box value) -> cell
///
/// Creates a mutable cell that can be modified with box-set!
pub fn prim_box(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "box requires exactly 1 argument, got {}",
            args.len()
        ));
    }

    Ok(Value::Cell(Rc::new(RefCell::new(Box::new(
        args[0].clone(),
    )))))
}

/// Extract the value from a cell
///
/// (unbox cell) -> value
///
/// Returns the current value stored in the cell
pub fn prim_unbox(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "unbox requires exactly 1 argument, got {}",
            args.len()
        ));
    }

    match &args[0] {
        Value::Cell(cell) | Value::LocalCell(cell) => {
            let borrowed = cell.borrow();
            Ok((**borrowed).clone())
        }
        other => Err(format!("unbox requires a cell, got {}", other.type_name())),
    }
}

/// Modify the value in a cell
///
/// (box-set! cell value) -> value
///
/// Sets the cell to contain the new value and returns the new value
pub fn prim_box_set(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err(format!(
            "box-set! requires exactly 2 arguments, got {}",
            args.len()
        ));
    }

    match &args[0] {
        Value::Cell(cell) | Value::LocalCell(cell) => {
            let mut borrowed = cell.borrow_mut();
            **borrowed = args[1].clone();
            Ok(args[1].clone())
        }
        other => Err(format!(
            "box-set! requires a cell as first argument, got {}",
            other.type_name()
        )),
    }
}

/// Check if a value is a box
///
/// (box? value) -> bool
///
/// Returns #t if the value is a box, #f otherwise
pub fn prim_box_p(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "box? requires exactly 1 argument, got {}",
            args.len()
        ));
    }

    Ok(Value::Bool(matches!(
        &args[0],
        Value::Cell(_) | Value::LocalCell(_)
    )))
}
