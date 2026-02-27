//! Cell/Box primitives for mutable storage
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Create a mutable cell containing a value
///
/// (box value) -> cell
///
/// Creates a mutable cell that can be modified with rebox
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
/// (rebox cell value) -> value
///
/// Sets the cell to contain the new value and returns the new value
pub fn prim_rebox(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("rebox: expected 2 arguments, got {}", args.len()),
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
                format!("rebox: expected cell, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Check if a value is a box
///
/// (box? value) -> bool
///
/// Returns true if the value is a box, false otherwise
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

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "box",
        func: prim_box,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Create a mutable cell containing a value.",
        params: &["value"],
        category: "cell",
        example: "(box 42) ;=> #<cell>",
        aliases: &[],
    },
    PrimitiveDef {
        name: "unbox",
        func: prim_unbox,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Extract the value from a cell.",
        params: &["cell"],
        category: "cell",
        example: "(unbox (box 42)) ;=> 42",
        aliases: &[],
    },
    PrimitiveDef {
        name: "rebox",
        func: prim_rebox,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Modify the value in a cell and return the new value.",
        params: &["cell", "value"],
        category: "cell",
        example: "(let ((c (box 1))) (rebox c 2) (unbox c)) ;=> 2",
        aliases: &[],
    },
    PrimitiveDef {
        name: "box?",
        func: prim_box_p,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if a value is a box.",
        params: &["value"],
        category: "cell",
        example: "(box? (box 1)) ;=> true\n(box? 42) ;=> false",
        aliases: &[],
    },
];
