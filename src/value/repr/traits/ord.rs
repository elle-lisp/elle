// audited: 2026-11-18
//! The one order of all values: by kind, then by a comparison within the kind.
//!
//! docs/types.md
//! docs/impl/values.md
//!
//! `Kind` places two values of two kinds. Within a kind the contents decide,
//! read through one view of the immutable and the mutable form, so a value
//! and its mutable twin compare `Equal`.

use super::kind::{stores_values, Kind};
use super::view::{with_bytes, with_elements, with_entries, with_members};
use super::*;
use crate::value::cycle;
use std::cmp::Ordering;

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        // The same 16 bytes are the same value.
        if self.tag == other.tag && self.payload == other.payload {
            return Ordering::Equal;
        }
        let kind = Kind::of(self);
        match kind.cmp(&Kind::of(other)) {
            Ordering::Equal => cmp_same_kind(self, other, kind),
            ord => ord,
        }
    }
}

/// Compare two numbers by value. Two integers compare exactly, and any other
/// pair compares as `f64`. `-0.0` orders before `0.0`. Every NaN orders before
/// every other number, whatever its sign bit, and two NaNs order by their bit
/// patterns: each is its own key, as `PartialEq` has it.
fn cmp_numbers(a: &Value, b: &Value) -> Ordering {
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return x.cmp(&y);
    }
    let x = a.as_number().expect("a value of the number kind");
    let y = b.as_number().expect("a value of the number kind");
    match (x.is_nan(), y.is_nan()) {
        (false, false) => x.total_cmp(&y),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (true, true) => x.to_bits().cmp(&y.to_bits()),
    }
}

/// Run `f` unless `a` against `b` is already being compared further up the
/// stack. A cycle of values runs through a container that stores values on
/// each side, so a pair with none needs no guard. A pair met again is `Equal`:
/// the comparison that first met the pair decides.
fn unless_cyclic(a: &Value, b: &Value, f: impl FnOnce() -> Ordering) -> Ordering {
    if !stores_values(a) && !stores_values(b) {
        return f();
    }
    match cycle::cmp_enter(a.payload as usize, b.payload as usize) {
        Some(_guard) => f(),
        None => Ordering::Equal,
    }
}

/// Compare two values of one kind.
fn cmp_same_kind(a: &Value, b: &Value, kind: Kind) -> Ordering {
    use crate::value::heap::{deref, HeapObject};

    match kind {
        Kind::Nil | Kind::EmptyList => Ordering::Equal,
        Kind::Bool => a.as_bool().cmp(&b.as_bool()),
        Kind::Number => cmp_numbers(a, b),

        // A symbol and a keyword order by name hash, a C pointer by address
        // and a native function by its id. Each is the payload.
        Kind::Symbol | Kind::Keyword | Kind::Pointer | Kind::NativeFn => a.payload.cmp(&b.payload),

        Kind::String | Kind::Bytes => with_bytes(a, |x| with_bytes(b, |y| x.cmp(y))),
        Kind::Array => unless_cyclic(a, b, || {
            with_elements(a, |x| with_elements(b, |y| x.cmp(y)))
        }),
        Kind::Struct => unless_cyclic(a, b, || with_entries(a, |x| with_entries(b, |y| x.cmp(y)))),
        Kind::Set => unless_cyclic(a, b, || with_members(a, |x| with_members(b, |y| x.cmp(y)))),

        // SAFETY: every kind below is a heap kind, so both values point at
        // heap objects.
        _ => match unsafe { (deref(*a), deref(*b)) } {
            (HeapObject::Pair(x), HeapObject::Pair(y)) => x.cmp(y),

            // A box orders by the value it holds. A user box against a
            // capture cell falls to the address below.
            (HeapObject::LBox { cell: x, .. }, HeapObject::LBox { cell: y, .. })
            | (HeapObject::CaptureCell { cell: x, .. }, HeapObject::CaptureCell { cell: y, .. }) => {
                unless_cyclic(a, b, || x.get().cmp(&y.get()))
            }

            (HeapObject::LibHandle(x), HeapObject::LibHandle(y)) => x.cmp(y),

            // A fiber, a thread and an external each wrap a handle that two
            // slots can name, so each orders by the handle, as `PartialEq`
            // equates them (repr/eq.rs).
            (HeapObject::Fiber { handle: x, .. }, HeapObject::Fiber { handle: y, .. }) => {
                x.id().cmp(&y.id())
            }
            (
                HeapObject::ThreadHandle { handle: x, .. },
                HeapObject::ThreadHandle { handle: y, .. },
            ) => (std::sync::Arc::as_ptr(&x.result) as usize)
                .cmp(&(std::sync::Arc::as_ptr(&y.result) as usize)),
            (HeapObject::External { obj: x, .. }, HeapObject::External { obj: y, .. }) => {
                (std::rc::Rc::as_ptr(&x.data) as *const () as usize)
                    .cmp(&(std::rc::Rc::as_ptr(&y.data) as *const () as usize))
            }

            // Every other value is the object at its address.
            _ => a.payload.cmp(&b.payload),
        },
    }
}
// Debug is implemented in display.rs alongside Display, since both
// share the resolve_name helper for symbol/keyword resolution.
