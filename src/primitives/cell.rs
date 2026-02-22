//! Cell/Box primitives for mutable storage
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};

/// Create a mutable cell containing a value
///
/// (box value) -> cell
///
/// Creates a mutable cell that can be modified with box-set!
pub fn prim_box(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("box: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    (SIG_OK, Value::cell(args[0]))
}

/// Extract the value from a cell
///
/// (unbox cell) -> value
///
/// Returns the current value stored in the cell
pub fn prim_unbox(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("unbox: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    if let Some(cell) = args[0].as_cell() {
        let borrowed = cell.borrow();
        (SIG_OK, *borrowed)
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("unbox: expected cell, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Modify the value in a cell
///
/// (box-set! cell value) -> value
///
/// Sets the cell to contain the new value and returns the new value
pub fn prim_box_set(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("box-set!: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    if let Some(cell) = args[0].as_cell() {
        let mut borrowed = cell.borrow_mut();
        *borrowed = args[1];
        (SIG_OK, args[1])
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("box-set!: expected cell, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Check if a value is a box
///
/// (box? value) -> bool
///
/// Returns #t if the value is a box, #f otherwise
pub fn prim_box_p(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("box?: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    (SIG_OK, Value::bool(args[0].is_cell()))
}
