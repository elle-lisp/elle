//! Proptest strategies for generating arbitrary Elle Values.

#![allow(dead_code)]

use elle::ffi::types::{StructDesc, TypeDesc};
use elle::value::repr::{INT_MAX, INT_MIN};
use elle::Value;
use proptest::prelude::*;

/// Strategy for arbitrary immediate Values (no heap allocation).
///
/// Generates: nil, empty_list, true, false, integers, floats, symbols.
/// Does NOT generate: strings, cons, arrays, tables, closures, fibers
/// (heap types that require special handling or are reference-compared).
pub fn arb_immediate() -> impl Strategy<Value = Value> {
    prop_oneof![
        // Constants (weighted low — only a few possible values)
        1 => Just(Value::NIL),
        1 => Just(Value::EMPTY_LIST),
        1 => Just(Value::TRUE),
        1 => Just(Value::FALSE),
        // Integers (weighted high — large input space)
        10 => (INT_MIN..=INT_MAX).prop_map(Value::int),
        // Floats (weighted high — large input space)
        10 => prop::num::f64::NORMAL.prop_map(Value::float),
        // Float edge cases
        1 => Just(Value::float(0.0)),
        1 => Just(Value::float(-0.0)),
        1 => Just(Value::float(f64::INFINITY)),
        1 => Just(Value::float(f64::NEG_INFINITY)),
        1 => Just(Value::float(f64::NAN)),
        // Symbols
        3 => (0u32..10000).prop_map(Value::symbol),
    ]
}

/// Strategy for arbitrary Values including heap-allocated types.
///
/// Generates everything from `arb_immediate()` plus:
/// strings, cons cells, arrays (up to depth limit).
///
/// Heap values are compared by content (strings, cons, arrays)
/// not by reference (closures, fibers, native fns).
pub fn arb_value() -> impl Strategy<Value = Value> {
    arb_value_depth(3)
}

/// Strategy for arbitrary Values with bounded nesting depth.
fn arb_value_depth(depth: u32) -> BoxedStrategy<Value> {
    if depth == 0 {
        // Base case: only leaf values
        prop_oneof![
            2 => Just(Value::NIL),
            2 => Just(Value::EMPTY_LIST),
            1 => Just(Value::TRUE),
            1 => Just(Value::FALSE),
            10 => (INT_MIN..=INT_MAX).prop_map(Value::int),
            10 => prop::num::f64::NORMAL.prop_map(Value::float),
            1 => Just(Value::float(f64::INFINITY)),
            1 => Just(Value::float(f64::NEG_INFINITY)),
            3 => (0u32..10000).prop_map(Value::symbol),
            5 => "[a-zA-Z0-9_ ]{0,20}".prop_map(Value::string),
            1 => Just(Value::keyword("test")),
            1 => Just(Value::keyword("a")),
            1 => Just(Value::keyword("key")),
        ]
        .boxed()
    } else {
        let leaf = arb_value_depth(0);
        let inner = arb_value_depth(depth - 1);
        prop_oneof![
            // Leaf values (high weight to avoid explosion)
            10 => leaf,
            // Cons cells
            2 => (inner.clone(), arb_value_depth(depth - 1))
                .prop_map(|(car, cdr)| Value::cons(car, cdr)),
            // Proper lists (0-5 elements)
            2 => prop::collection::vec(arb_value_depth(depth - 1), 0..=5)
                .prop_map(|elems| {
                    elems.into_iter().rev().fold(Value::EMPTY_LIST, |acc, v| Value::cons(v, acc))
                }),
            // Arrays (0-5 elements)
            1 => prop::collection::vec(arb_value_depth(depth - 1), 0..=5)
                .prop_map(Value::array),
        ]
        .boxed()
    }
}

// =========================================================================
// FFI Type Strategies
// =========================================================================

/// Strategy for generating a primitive (non-compound) TypeDesc.
pub fn arb_primitive_type() -> impl Strategy<Value = TypeDesc> {
    prop_oneof![
        Just(TypeDesc::I8),
        Just(TypeDesc::U8),
        Just(TypeDesc::I16),
        Just(TypeDesc::U16),
        Just(TypeDesc::I32),
        Just(TypeDesc::U32),
        Just(TypeDesc::I64),
        Just(TypeDesc::U64),
        Just(TypeDesc::Float),
        Just(TypeDesc::Double),
        Just(TypeDesc::Ptr),
    ]
}

/// Strategy for generating a TypeDesc including compound types (up to given depth).
pub fn arb_type_desc(depth: u32) -> BoxedStrategy<TypeDesc> {
    if depth == 0 {
        arb_primitive_type().boxed()
    } else {
        prop_oneof![
            10 => arb_primitive_type(),
            // Struct with 1-4 fields
            2 => prop::collection::vec(arb_type_desc(depth - 1), 1..=4)
                .prop_map(|fields| TypeDesc::Struct(StructDesc { fields })),
            // Array with 1-4 elements
            1 => (arb_type_desc(depth - 1), 1usize..=4)
                .prop_map(|(elem, count)| TypeDesc::Array(Box::new(elem), count)),
        ]
        .boxed()
    }
}

/// Strategy for generating a StructDesc with 1-6 primitive fields.
pub fn arb_flat_struct() -> impl Strategy<Value = StructDesc> {
    prop::collection::vec(arb_primitive_type(), 1..=6).prop_map(|fields| StructDesc { fields })
}

/// Generate a Value that matches a given TypeDesc.
/// Returns a strategy that produces values valid for the type.
pub fn arb_value_for_type(desc: &TypeDesc) -> BoxedStrategy<Value> {
    match desc {
        TypeDesc::I8 => (-128i64..=127).prop_map(Value::int).boxed(),
        TypeDesc::U8 => (0i64..=255).prop_map(Value::int).boxed(),
        TypeDesc::I16 => (-32768i64..=32767).prop_map(Value::int).boxed(),
        TypeDesc::U16 => (0i64..=65535).prop_map(Value::int).boxed(),
        TypeDesc::I32 => (i32::MIN as i64..=i32::MAX as i64)
            .prop_map(Value::int)
            .boxed(),
        TypeDesc::U32 => (0i64..=u32::MAX as i64).prop_map(Value::int).boxed(),
        TypeDesc::I64 => (INT_MIN..=INT_MAX).prop_map(Value::int).boxed(),
        TypeDesc::U64 => (0i64..=INT_MAX).prop_map(Value::int).boxed(),
        TypeDesc::Float => prop::num::f64::NORMAL.prop_map(Value::float).boxed(),
        TypeDesc::Double => prop::num::f64::NORMAL.prop_map(Value::float).boxed(),
        TypeDesc::Ptr => prop_oneof![
            1 => Just(Value::NIL),
            3 => (1usize..=0x0000_FFFF_FFFF_FFFFusize).prop_map(Value::pointer),
        ]
        .boxed(),
        TypeDesc::Struct(sd) => {
            let field_strats: Vec<BoxedStrategy<Value>> =
                sd.fields.iter().map(arb_value_for_type).collect();
            field_strats.prop_map(Value::array).boxed()
        }
        TypeDesc::Array(elem, count) => {
            let count = *count;
            let elem_strat = arb_value_for_type(elem);
            prop::collection::vec(elem_strat, count..=count)
                .prop_map(Value::array)
                .boxed()
        }
        // For types we don't generate values for (Void, Str, etc.), just use a dummy
        _ => Just(Value::int(0)).boxed(),
    }
}

/// Generate a (TypeDesc, Value) pair where the value matches the type.
/// Only generates types that are safe for roundtrip testing (no strings,
/// no void, no platform-dependent types that might cause confusion).
pub fn arb_typed_value() -> BoxedStrategy<(TypeDesc, Value)> {
    arb_primitive_type()
        .prop_flat_map(|desc| {
            let strat = arb_value_for_type(&desc);
            strat.prop_map(move |val| (desc.clone(), val))
        })
        .boxed()
}

/// Generate a (StructDesc, Value::array) pair where the array matches the struct.
pub fn arb_struct_and_values() -> BoxedStrategy<(StructDesc, Value)> {
    arb_flat_struct()
        .prop_flat_map(|sd| {
            let field_strats: Vec<BoxedStrategy<Value>> =
                sd.fields.iter().map(arb_value_for_type).collect();
            let sd_clone = sd.clone();
            field_strats.prop_map(move |vals| (sd_clone.clone(), Value::array(vals)))
        })
        .boxed()
}
