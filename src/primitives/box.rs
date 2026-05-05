//! Box primitives for mutable storage
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Create a mutable box containing a value
///
/// (box value) -> box
///
/// Creates a mutable box that can be modified with rebox
pub(crate) fn prim_box(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::lbox(args[0]))
}

/// Extract the value from a box
///
/// (unbox box) -> value
///
/// Returns the current value stored in the box
pub(crate) fn prim_unbox(args: &[Value]) -> (SignalBits, Value) {
    if let Some(cell) = args[0].as_lbox() {
        let borrowed = cell.borrow();
        (SIG_OK, *borrowed)
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("unbox: expected box, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Modify the value in a box
///
/// (rebox box value) -> value
///
/// Sets the box to contain the new value and returns the new value
pub(crate) fn prim_rebox(args: &[Value]) -> (SignalBits, Value) {
    if let Some(cell) = args[0].as_lbox() {
        let mut borrowed = cell.borrow_mut();
        *borrowed = args[1];
        (SIG_OK, args[1])
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("rebox: expected box, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Check if a value is a box
///
/// (box? value) -> bool
///
/// Returns true if the value is a box, false otherwise
pub(crate) fn prim_box_p(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::bool(args[0].is_lbox()))
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "box",
        func: prim_box,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create a mutable box containing a value.",
        params: &["value"],
        category: "box",
        example: "(box 42) #=> #<box>",
        aliases: &[],
    },
    PrimitiveDef {
        name: "unbox",
        func: prim_unbox,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Extract the value from a box.",
        params: &["box"],
        category: "box",
        example: "(unbox (box 42)) #=> 42",
        aliases: &[],
    },
    PrimitiveDef {
        name: "rebox",
        func: prim_rebox,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Modify the value in a box and return the new value.",
        params: &["box", "value"],
        category: "box",
        example: "(let [c (box 1)] (rebox c 2) (unbox c)) #=> 2",
        aliases: &[],
    },
    PrimitiveDef {
        name: "box?",
        func: prim_box_p,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if a value is a box.",
        params: &["value"],
        category: "predicate",
        example: "(box? (box 1)) #=> true\n(box? 42) #=> false",
        aliases: &[],
    },
];
