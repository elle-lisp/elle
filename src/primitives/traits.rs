//! Trait table primitives: `with-traits` and `traits`.
//!
//! `with-traits` attaches an immutable struct as a trait table to a value,
//! returning a new heap object with the same data and the given table.
//!
//! `traits` returns the trait table attached to a value, or `nil` if none.

use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::heap::{alloc, deref, HeapObject};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// (with-traits value table) → new value with trait table attached
///
/// - value must be one of the 19 traitable heap types
/// - table must be an immutable struct (LStruct)
/// - returns a new heap object with the same data and traits = table
/// - for mutable types, data storage is shared (same RefCell)
pub(crate) fn prim_with_traits(args: &[Value]) -> (SignalBits, Value) {
    let value = args[0];
    let table = args[1];

    // Validate: value must be a heap-allocated traitable type
    if !value.is_heap() {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "with-traits: value must be a traitable heap type, got {}",
                    value.type_name()
                ),
            ),
        );
    }

    // Validate: table must be a struct (LStruct or LStructMut)
    if !table.is_heap() || {
        let tag = unsafe { deref(table) }.tag();
        tag != crate::value::heap::HeapTag::LStruct
            && tag != crate::value::heap::HeapTag::LStructMut
    } {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "with-traits: trait table must be a struct, got {}",
                    table.type_name()
                ),
            ),
        );
    }

    // Clone the heap object with new traits
    match unsafe { clone_with_traits(value, table) } {
        Ok(v) => (SIG_OK, v),
        Err(msg) => (SIG_ERROR, error_val("type-error", msg)),
    }
}

/// Clone a heap value, replacing the traits field with `table`.
///
/// For mutable types (LArrayMut, LStructMut, LStringMut, LBytesMut, LSetMut,
/// LBox), the data is shared (same RefCell Rc). Mutations to the original are
/// visible through the traited copy.
///
/// For infrastructure types (Float, NativeFn, LibHandle, FFISignature,
/// FFIType), returns Err.
///
/// # Safety
/// `value` must be a valid heap pointer.
unsafe fn clone_with_traits(value: Value, table: Value) -> Result<Value, String> {
    match deref(value) {
        HeapObject::LString { s, .. } => Ok(alloc(HeapObject::LString {
            s: *s,
            traits: table,
        })),
        HeapObject::Pair(pair) => Ok(alloc(HeapObject::Pair(crate::value::heap::Pair {
            first: pair.first,
            rest: pair.rest,
            traits: table,
        }))),
        // Mutable collections wrap their backing storage in Rc<RefCell<_>>
        // so cross-fiber sharing survives deep_copy_to_outbox. But
        // with-traits is documented as returning an INDEPENDENT copy: a
        // push to the original must not be visible through the traited
        // copy. So we materialize a fresh Rc with a cloned inner value.
        HeapObject::LArrayMut { data, .. } => Ok(alloc(HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(data.borrow().clone())),
            traits: table,
        })),
        HeapObject::LStructMut { data, .. } => Ok(alloc(HeapObject::LStructMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(data.borrow().clone())),
            traits: table,
        })),
        HeapObject::LStruct { data, .. } => Ok(alloc(HeapObject::LStruct {
            data: data.clone(),
            traits: table,
        })),
        HeapObject::Closure { closure, .. } => Ok(alloc(HeapObject::Closure {
            closure: closure.clone(),
            traits: table,
        })),
        HeapObject::LArray { elements, .. } => Ok(alloc(HeapObject::LArray {
            elements: *elements,
            traits: table,
        })),
        HeapObject::LStringMut { data, .. } => Ok(alloc(HeapObject::LStringMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(data.borrow().clone())),
            traits: table,
        })),
        HeapObject::LBytes { data, .. } => Ok(alloc(HeapObject::LBytes {
            data: *data,
            traits: table,
        })),
        HeapObject::LBytesMut { data, .. } => Ok(alloc(HeapObject::LBytesMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(data.borrow().clone())),
            traits: table,
        })),
        HeapObject::LBox { cell, .. } => Ok(alloc(HeapObject::LBox {
            cell: std::rc::Rc::new(std::cell::RefCell::new(*cell.borrow())),
            traits: table,
        })),
        HeapObject::CaptureCell { cell, .. } => Ok(alloc(HeapObject::CaptureCell {
            cell: std::rc::Rc::new(std::cell::RefCell::new(*cell.borrow())),
            traits: table,
        })),
        HeapObject::Fiber { handle, .. } => Ok(alloc(HeapObject::Fiber {
            handle: handle.clone(),
            traits: table,
        })),
        HeapObject::Syntax { syntax, .. } => Ok(alloc(HeapObject::Syntax {
            syntax: syntax.clone(),
            traits: table,
        })),
        HeapObject::ManagedPointer { addr, .. } => Ok(alloc(HeapObject::ManagedPointer {
            addr: std::cell::Cell::new(addr.get()),
            traits: table,
        })),
        HeapObject::External { obj, .. } => Ok(alloc(HeapObject::External {
            obj: obj.clone(),
            traits: table,
        })),
        HeapObject::Parameter { id, default, .. } => Ok(alloc(HeapObject::Parameter {
            id: *id,
            default: *default,
            traits: table,
        })),
        HeapObject::ThreadHandle { handle, .. } => Ok(alloc(HeapObject::ThreadHandle {
            handle: handle.clone(),
            traits: table,
        })),
        HeapObject::LSet { data, .. } => Ok(alloc(HeapObject::LSet {
            data: *data,
            traits: table,
        })),
        HeapObject::LSetMut { data, .. } => Ok(alloc(HeapObject::LSetMut {
            data: data.clone(),
            traits: table,
        })),
        // Infrastructure types — no trait field; return error
        HeapObject::Float(_)
        | HeapObject::NativeFn(_)
        | HeapObject::LibHandle(_)
        | HeapObject::FFISignature(_, _)
        | HeapObject::FFIType(_) => Err(format!(
            "with-traits: cannot attach traits to infrastructure type {}",
            deref(value).type_name()
        )),
    }
}

/// (traits value) → trait table or nil
///
/// Returns the trait table attached to value. Since traits are stamped at
/// allocation for collection types, this simply reads the traits field.
/// Returns nil for immediates and infrastructure types.
pub(crate) fn prim_traits(args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        crate::primitives::traitregistry::get_traitset(&args[0]),
    )
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "with-traits",
        func: prim_with_traits,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Attach a trait table to a value. Returns a new value with the same data and the given trait table. The table must be a struct (immutable or mutable).",
        params: &["value", "table"],
        category: "traits",
        example: "(with-traits [1 2 3] {:Seq {:first (fn (v) (get v 0))}})",
        aliases: &[],
    },
    PrimitiveDef {
        name: "traits",
        func: prim_traits,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the trait table attached to a value, or nil if none. Usable as boolean: (if (traits v) ...) checks for presence.",
        params: &["value"],
        category: "traits",
        example: "(traits (with-traits [1 2 3] {:Seq {:first (fn (v) (get v 0))}}))",
        aliases: &[],
    },
];
