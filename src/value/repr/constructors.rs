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

    /// Create a string value (heap-allocated, bytes stored inline in arena).
    #[inline]
    pub fn string(s: impl AsRef<str>) -> Self {
        use crate::value::arena::alloc_inline_slice;
        use crate::value::heap::{alloc, HeapObject};
        let bytes = s.as_ref().as_bytes();
        let slice = alloc_inline_slice::<u8>(bytes);
        alloc(HeapObject::LString {
            s: slice,
            traits: Value::NIL,
        })
    }

    /// Create a cons cell.
    #[inline]
    pub fn pair(head: Value, tail: Value) -> Self {
        use crate::value::heap::{alloc, HeapObject, Pair};
        alloc(HeapObject::Pair(Pair {
            first: head,
            rest: tail,
            traits: Value::NIL,
        }))
    }

    /// Create a mutable @array.
    #[inline]
    pub fn array_mut(elements: Vec<Value>) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        use std::rc::Rc;
        alloc(HeapObject::LArrayMut {
            data: Rc::new(RefCell::new(elements)),
            traits: Value::NIL,
        })
    }

    /// Create an empty mutable @struct.
    #[inline]
    pub fn struct_mut() -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        use std::collections::BTreeMap;
        use std::rc::Rc;
        alloc(HeapObject::LStructMut {
            data: Rc::new(RefCell::new(BTreeMap::new())),
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
        use std::rc::Rc;
        alloc(HeapObject::LStructMut {
            data: Rc::new(RefCell::new(entries)),
            traits: Value::NIL,
        })
    }

    /// Create an immutable struct from a BTreeMap (entries will be sorted).
    #[inline]
    pub fn struct_from(
        fields: std::collections::BTreeMap<crate::value::heap::TableKey, Value>,
    ) -> Self {
        // BTreeMap iterates in sorted order, so Vec is already sorted.
        let sorted: Vec<(crate::value::heap::TableKey, Value)> = fields.into_iter().collect();
        Self::struct_from_sorted(sorted)
    }

    /// Create an immutable struct from a pre-sorted Vec of key-value pairs.
    ///
    /// Keeps the Vec on the Rust heap because TableKey::String carries an
    /// owned allocation; arena memcpy would leak or double-free the String.
    ///
    /// # Safety contract
    /// Caller must ensure entries are sorted by key and contain no duplicates.
    #[inline]
    pub fn struct_from_sorted(entries: Vec<(crate::value::heap::TableKey, Value)>) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::LStruct {
            data: entries,
            traits: Value::NIL,
        })
    }

    /// Create a closure. The closure is stored by value inline in the arena;
    /// `Closure`'s non-Copy fields are `Rc`-shared (`ClosureTemplate`), so
    /// cloning on access is O(1).
    #[inline]
    pub fn closure(c: crate::value::heap::Closure) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::Closure {
            closure: c,
            traits: Value::NIL,
        })
    }

    /// Create a user box (mutable LBox) — NOT auto-unwrapped by LoadUpvalue.
    #[inline]
    pub fn lbox(value: Value) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        use std::rc::Rc;
        alloc(HeapObject::LBox {
            cell: Rc::new(RefCell::new(value)),
            traits: Value::NIL,
        })
    }

    /// Create a compiler capture cell — auto-unwrapped by LoadUpvalue.
    /// Used for mutable captured variables.
    #[inline]
    pub fn capture_cell(value: Value) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        use std::rc::Rc;
        alloc(HeapObject::CaptureCell {
            cell: Rc::new(RefCell::new(value)),
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

    /// Create an immutable array value (elements stored inline in arena).
    #[inline]
    pub fn array(elements: Vec<Value>) -> Self {
        use crate::value::arena::alloc_inline_slice;
        use crate::value::heap::{alloc, HeapObject};
        let slice = alloc_inline_slice::<Value>(&elements);
        alloc(HeapObject::LArray {
            elements: slice,
            traits: Value::NIL,
        })
    }

    /// Create a mutable @string value.
    #[inline]
    pub fn string_mut(bytes: Vec<u8>) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        use std::rc::Rc;
        alloc(HeapObject::LStringMut {
            data: Rc::new(RefCell::new(bytes)),
            traits: Value::NIL,
        })
    }

    /// Create an immutable bytes value (stored inline in arena).
    #[inline]
    pub fn bytes(data: Vec<u8>) -> Self {
        use crate::value::arena::alloc_inline_slice;
        use crate::value::heap::{alloc, HeapObject};
        let slice = alloc_inline_slice::<u8>(&data);
        alloc(HeapObject::LBytes {
            data: slice,
            traits: Value::NIL,
        })
    }

    /// Create a mutable @bytes value.
    #[inline]
    pub fn bytes_mut(data: Vec<u8>) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        use std::rc::Rc;
        alloc(HeapObject::LBytesMut {
            data: Rc::new(RefCell::new(data)),
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
        alloc(HeapObject::Syntax {
            syntax: Box::new(s),
            traits: Value::NIL,
        })
    }

    /// Create an FFI signature value.
    #[inline]
    pub fn ffi_signature(sig: crate::ffi::types::Signature) -> Self {
        use crate::value::heap::{alloc, CifCache, HeapObject};
        #[cfg(feature = "ffi")]
        let cache: CifCache = std::cell::RefCell::new(None);
        #[cfg(not(feature = "ffi"))]
        let cache: CifCache = ();
        alloc(HeapObject::FFISignature(sig, cache))
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

    /// Create an immutable set value (sorted elements stored inline in arena).
    #[inline]
    pub fn set(items: BTreeSet<Value>) -> Self {
        use crate::value::arena::alloc_inline_slice;
        use crate::value::heap::{alloc, HeapObject};
        // BTreeSet iterates in sorted order; collect into Vec and copy into arena.
        let sorted: Vec<Value> = items.into_iter().collect();
        let slice = alloc_inline_slice::<Value>(&sorted);
        alloc(HeapObject::LSet {
            data: slice,
            traits: Value::NIL,
        })
    }

    /// Create a mutable set value.
    #[inline]
    pub fn set_mut(items: BTreeSet<Value>) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        use std::rc::Rc;
        alloc(HeapObject::LSetMut {
            data: Rc::new(RefCell::new(items)),
            traits: Value::NIL,
        })
    }
}
