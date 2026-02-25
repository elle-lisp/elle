//! Proptest strategies for generating arbitrary Elle Values.

#![allow(dead_code)]

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
