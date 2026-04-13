//! Value constructors for immediate and heap-allocated types.

use std::any::Any;
use std::collections::BTreeSet;

use super::{Value, TAG_CPOINTER, TAG_FLOAT, TAG_INT, TAG_KEYWORD, TAG_SYMBOL};

impl Value {
    // =========================================================================
    // Immediate Value Constructors
    // =========================================================================

    /// Create an integer value.
    #[inline]
    pub fn int(n: i64) -> Self {
        Value {
            tag: TAG_INT,
            payload: n as u64,
        }
    }

    /// Create a float value.
    #[inline]
    pub fn float(f: f64) -> Self {
        Value {
            tag: TAG_FLOAT,
            payload: f.to_bits(),
        }
    }

    /// Create a boolean value.
    #[inline]
    pub fn bool(b: bool) -> Self {
        if b {
            Self::TRUE
        } else {
            Self::FALSE
        }
    }

    /// Create a symbol value from a SymbolId.
    #[inline]
    pub fn symbol(id: u32) -> Self {
        Value {
            tag: TAG_SYMBOL,
            payload: id as u64,
        }
    }

    /// Create a keyword value from a name string.
    /// The name is hashed and registered in the global keyword table.
    /// Equality is O(1) hash comparison; name recovery via `as_keyword_name()`.
    #[inline]
    pub fn keyword(name: &str) -> Self {
        let hash = crate::value::keyword::intern_keyword(name);
        Value {
            tag: TAG_KEYWORD,
            payload: hash,
        }
    }

    /// Create a raw C pointer value.
    ///
    /// NULL pointers (address 0) are represented as `Value::NIL`.
    /// This is an immediate value, not heap-allocated.
    #[inline]
    pub fn pointer(addr: usize) -> Self {
        if addr == 0 {
            return Self::NIL;
        }
        Value {
            tag: TAG_CPOINTER,
            payload: addr as u64,
        }
    }

    /// Create an empty list value.
    #[inline]
    pub fn empty_list() -> Self {
        Self::EMPTY_LIST
    }

    // =========================================================================
    // Heap Value Constructors
    // =========================================================================

    /// Create a string value (heap-allocated).
    #[inline]
    pub fn string(s: impl Into<Box<str>>) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        let boxed: Box<str> = s.into();
        alloc(HeapObject::LString {
            s: boxed,
            traits: Value::NIL,
        })
    }

    /// Create a heap string without interning. Used by `SendValue::into_value()`
    /// to avoid thread-local interner issues when reconstructing values on
    /// a different thread.
    #[inline]
    pub fn string_no_intern(s: impl Into<Box<str>>) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        let boxed: Box<str> = s.into();
        alloc(HeapObject::LString {
            s: boxed,
            traits: Value::NIL,
        })
    }

    /// Create a cons cell.
    #[inline]
    pub fn cons(car: Value, cdr: Value) -> Self {
        use crate::value::heap::{alloc, Cons, HeapObject};
        alloc(HeapObject::Cons(Cons {
            first: car,
            rest: cdr,
            traits: Value::NIL,
        }))
    }

    /// Create a mutable @array.
    #[inline]
    pub fn array_mut(elements: Vec<Value>) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        alloc(HeapObject::LArrayMut {
            data: RefCell::new(elements),
            traits: Value::NIL,
        })
    }

    /// Create an empty mutable @struct.
    #[inline]
    pub fn struct_mut() -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        use std::collections::BTreeMap;
        alloc(HeapObject::LStructMut {
            data: RefCell::new(BTreeMap::new()),
            traits: Value::NIL,
        })
    }

    /// Create an @struct with initial entries.
    #[inline]
    pub fn struct_mut_from(
        entries: std::collections::BTreeMap<crate::value::heap::TableKey, Value>,
    ) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        alloc(HeapObject::LStructMut {
            data: RefCell::new(entries),
            traits: Value::NIL,
        })
    }

    /// Create an immutable struct.
    #[inline]
    pub fn struct_from(
        fields: std::collections::BTreeMap<crate::value::heap::TableKey, Value>,
    ) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::LStruct {
            data: fields,
            traits: Value::NIL,
        })
    }

    /// Create a closure.
    #[inline]
    pub fn closure(c: crate::value::heap::Closure) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::rc::Rc;
        alloc(HeapObject::Closure {
            closure: Rc::new(c),
            traits: Value::NIL,
        })
    }

    /// Create a user box (mutable LBox) — NOT auto-unwrapped by LoadUpvalue.
    #[inline]
    pub fn lbox(value: Value) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        alloc(HeapObject::LBox {
            cell: RefCell::new(value),
            traits: Value::NIL,
        })
    }

    /// Create a compiler capture cell — auto-unwrapped by LoadUpvalue.
    /// Used for mutable captured variables.
    #[inline]
    pub fn capture_cell(value: Value) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        alloc(HeapObject::CaptureCell {
            cell: RefCell::new(value),
            traits: Value::NIL,
        })
    }

    /// Create a native function value from a static primitive definition.
    /// Uses permanent allocation — native functions outlive any arena scope.
    #[inline]
    pub fn native_fn(def: &'static crate::primitives::def::PrimitiveDef) -> Self {
        use crate::value::heap::{alloc_permanent, HeapObject};
        alloc_permanent(HeapObject::NativeFn(def))
    }

    /// Create an immutable array value.
    #[inline]
    pub fn array(elements: Vec<Value>) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::LArray {
            elements,
            traits: Value::NIL,
        })
    }

    /// Create a mutable @string value.
    #[inline]
    pub fn string_mut(bytes: Vec<u8>) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        alloc(HeapObject::LStringMut {
            data: RefCell::new(bytes),
            traits: Value::NIL,
        })
    }

    /// Create an immutable bytes value.
    #[inline]
    pub fn bytes(data: Vec<u8>) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::LBytes {
            data,
            traits: Value::NIL,
        })
    }

    /// Create a mutable @bytes value.
    #[inline]
    pub fn bytes_mut(data: Vec<u8>) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        alloc(HeapObject::LBytesMut {
            data: RefCell::new(data),
            traits: Value::NIL,
        })
    }

    /// Create a fiber value.
    #[inline]
    pub fn fiber(f: crate::value::fiber::Fiber) -> Self {
        use crate::value::fiber::FiberHandle;
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::Fiber {
            handle: FiberHandle::new(f),
            traits: Value::NIL,
        })
    }

    /// Create a fiber value from an existing FiberHandle.
    #[inline]
    pub fn fiber_from_handle(handle: crate::value::fiber::FiberHandle) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::Fiber {
            handle,
            traits: Value::NIL,
        })
    }

    /// Create a syntax object value.
    /// Preserves scope sets through the Value round-trip during macro expansion.
    #[inline]
    pub fn syntax(s: crate::syntax::Syntax) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::rc::Rc;
        alloc(HeapObject::Syntax {
            syntax: Rc::new(s),
            traits: Value::NIL,
        })
    }

    /// Create an FFI signature value.
    #[inline]
    pub fn ffi_signature(sig: crate::ffi::types::Signature) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        alloc(HeapObject::FFISignature(sig, RefCell::new(None)))
    }

    /// Create an FFI compound type descriptor value.
    ///
    /// Only for compound types (Struct, Array). Primitive types use keywords.
    #[inline]
    pub fn ffi_type(desc: crate::ffi::types::TypeDesc) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        debug_assert!(
            matches!(
                desc,
                crate::ffi::types::TypeDesc::Struct(_) | crate::ffi::types::TypeDesc::Array(_, _)
            ),
            "FFIType should only wrap compound types"
        );
        alloc(HeapObject::FFIType(desc))
    }

    /// Create a library handle value.
    #[inline]
    pub fn lib_handle(id: u32) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::LibHandle(id))
    }

    /// Create a managed FFI pointer (tracks freed state).
    /// Used by ffi/malloc. NULL becomes nil (same as raw pointer).
    #[inline]
    pub fn managed_pointer(addr: usize) -> Self {
        if addr == 0 {
            return Self::NIL;
        }
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::Cell;
        alloc(HeapObject::ManagedPointer {
            addr: Cell::new(Some(addr)),
            traits: Value::NIL,
        })
    }

    /// Create an external object value (for plugin-provided types).
    #[inline]
    pub fn external<T: Any + 'static>(type_name: &'static str, data: T) -> Self {
        use crate::value::heap::{alloc, ExternalObject, HeapObject};
        use std::rc::Rc;
        alloc(HeapObject::External {
            obj: ExternalObject {
                type_name,
                data: Rc::new(data),
            },
            traits: Value::NIL,
        })
    }

    /// Create a dynamic parameter value.
    ///
    /// Each parameter gets a unique id from a global counter.
    /// The default value is returned when no `parameterize` binding is active.
    #[inline]
    pub fn parameter(default: Value) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT_ID: AtomicU32 = AtomicU32::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        alloc(HeapObject::Parameter {
            id,
            default,
            traits: Value::NIL,
        })
    }

    /// Create an immutable set value.
    #[inline]
    pub fn set(items: BTreeSet<Value>) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::LSet {
            data: items,
            traits: Value::NIL,
        })
    }

    /// Create a mutable set value.
    #[inline]
    pub fn set_mut(items: BTreeSet<Value>) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        alloc(HeapObject::LSetMut {
            data: RefCell::new(items),
            traits: Value::NIL,
        })
    }
}
