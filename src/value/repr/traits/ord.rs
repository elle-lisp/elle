use super::*;

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;

        // Fast path: identical values → Equal
        if self.tag == other.tag && self.payload == other.payload {
            return Ordering::Equal;
        }

        let self_rank = type_rank(self);
        let other_rank = type_rank(other);
        match self_rank.cmp(&other_rank) {
            Ordering::Equal => {}
            ord => return ord,
        }

        cmp_same_rank(self, other, self_rank)
    }
}

/// Assign a numeric rank to each value type for cross-type ordering.
///
/// Values with different ranks are ordered by rank alone.
/// Values with the same rank are compared within-type by `cmp_same_rank`.
fn type_rank(v: &Value) -> u8 {
    use crate::value::heap::{deref, HeapTag};

    if v.is_nil() {
        0
    } else if v.is_bool() {
        1
    } else if v.is_int() || v.is_float() {
        2
    } else if v.is_symbol() {
        3
    } else if v.is_keyword() {
        4
    } else if v.is_pointer() {
        5
    } else if v.is_empty_list() {
        6
    } else if v.is_native_fn() {
        // Native-fn is an immediate (tag below the heap boundary): rank 18, where
        // native-fns sat as heap objects before. Compared by prim_id below.
        18
    } else if v.is_heap() {
        match unsafe { deref(*v).tag() } {
            HeapTag::LString => 7,
            HeapTag::Pair => 8,
            HeapTag::LArray => 9,
            HeapTag::LArrayMut => 10,
            HeapTag::LBytes => 11,
            HeapTag::LStringMut => 12,
            HeapTag::LBytesMut => 13,
            HeapTag::LStruct => 14,
            HeapTag::LStructMut => 15,
            HeapTag::Closure => 16,
            HeapTag::LBox => 17,
            HeapTag::CaptureCell => 17, // same rank as LBox
            HeapTag::LibHandle => 19,
            HeapTag::ThreadHandle => 20,
            HeapTag::Fiber => 21,
            HeapTag::Syntax => 22,
            HeapTag::FFISignature => 23,
            HeapTag::FFIType => 24,
            HeapTag::ManagedPointer => 25,
            HeapTag::External => 26,
            HeapTag::Parameter => 27,
            HeapTag::LSet => 28,
            HeapTag::LSetMut => 29,
            // Closure template — never user-visible, so never legitimately
            // compared; give it a distinct rank so a stray comparison is total.
            HeapTag::ClosureTemplate => 31,
            // Float as heap object is a legacy variant; treat same rank as number.
            HeapTag::Float => 2,
        }
    } else {
        // Unknown — should not happen
        30
    }
}

/// Compare two values known to have the same type rank.
fn cmp_same_rank(a: &Value, b: &Value, rank: u8) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    match rank {
        // Nil — singleton
        0 => Ordering::Equal,

        // Bool — false < true
        1 => {
            let a_bool = a.as_bool().unwrap();
            let b_bool = b.as_bool().unwrap();
            a_bool.cmp(&b_bool)
        }

        // Number (int or float) — compare as f64, use total_cmp for NaN ordering
        2 => {
            // Fast path: both ints
            if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
                return ai.cmp(&bi);
            }
            // Mixed or both floats: coerce to f64
            let af = a.as_number().unwrap();
            let bf = b.as_number().unwrap();
            af.total_cmp(&bf)
        }

        // Symbol — by ID
        3 => {
            let a_id = a.as_symbol().unwrap();
            let b_id = b.as_symbol().unwrap();
            a_id.cmp(&b_id)
        }

        // Keyword — lexicographic by name
        4 => {
            let a_name = a.as_keyword_name().unwrap();
            let b_name = b.as_keyword_name().unwrap();
            a_name.cmp(&b_name)
        }

        // C pointer — by address (payload)
        5 => a.payload.cmp(&b.payload),

        // Empty list — singleton
        6 => Ordering::Equal,

        // String (heap) — lexicographic by content
        7 => a.compare_str(b).unwrap_or(Ordering::Equal),

        // Native-fn (immediate) — by prim_id (the payload IS the identity).
        18 => a.payload.cmp(&b.payload),

        // Heap types (ranks 9–31) — deref and compare
        _ => unsafe { cmp_heap(a, b) },
    }
}

/// Compare two heap values of the same type.
///
/// # Safety
/// Both values must be heap pointers (`is_heap()` returns true).
unsafe fn cmp_heap(a: &Value, b: &Value) -> std::cmp::Ordering {
    use crate::value::heap::{deref, HeapObject};
    use std::cmp::Ordering;

    // Pointer-identity fast path
    if a.payload == b.payload {
        return Ordering::Equal;
    }

    let a_obj = deref(*a);
    let b_obj = deref(*b);

    match (a_obj, b_obj) {
        // Pair — (first, rest) lexicographic (Pair::cmp ignores traits)
        (HeapObject::Pair(c1), HeapObject::Pair(c2)) => c1.cmp(c2),

        // Array — element-wise lexicographic
        (HeapObject::LArray { elements: t1, .. }, HeapObject::LArray { elements: t2, .. }) => {
            t1.cmp(t2)
        }

        // Array — element-wise lexicographic (borrow)
        (HeapObject::LArrayMut { data: a1, .. }, HeapObject::LArrayMut { data: a2, .. }) => {
            let _guard = match cycle::cmp_enter(a.payload as usize, b.payload as usize) {
                Some(g) => g,
                None => return Ordering::Equal,
            };
            let b1 = a1.borrow();
            let b2 = a2.borrow();
            b1.as_slice().cmp(b2.as_slice())
        }

        // Bytes — byte-wise lexicographic
        (HeapObject::LBytes { data: b1, .. }, HeapObject::LBytes { data: b2, .. }) => b1.cmp(b2),

        // @string — byte-wise lexicographic (borrow)
        (HeapObject::LStringMut { data: b1, .. }, HeapObject::LStringMut { data: b2, .. }) => {
            let r1 = b1.borrow();
            let r2 = b2.borrow();
            r1.cmp(&*r2)
        }

        // @bytes — byte-wise lexicographic (borrow)
        (HeapObject::LBytesMut { data: b1, .. }, HeapObject::LBytesMut { data: b2, .. }) => {
            let r1 = b1.borrow();
            let r2 = b2.borrow();
            r1.cmp(&*r2)
        }

        // Struct — entry-wise lexicographic (BTreeMap iteration is sorted)
        (HeapObject::LStruct { data: s1, .. }, HeapObject::LStruct { data: s2, .. }) => {
            s1.iter().cmp(s2.iter())
        }

        // @struct — entry-wise lexicographic (borrow)
        (HeapObject::LStructMut { data: t1, .. }, HeapObject::LStructMut { data: t2, .. }) => {
            let _guard = match cycle::cmp_enter(a.payload as usize, b.payload as usize) {
                Some(g) => g,
                None => return Ordering::Equal,
            };
            let b1 = t1.borrow();
            let b2 = t2.borrow();
            b1.iter().cmp(b2.iter())
        }

        // Box / CaptureCell — by contained value (borrow)
        (HeapObject::LBox { cell: c1, .. }, HeapObject::LBox { cell: c2, .. })
        | (HeapObject::CaptureCell { cell: c1, .. }, HeapObject::CaptureCell { cell: c2, .. }) => {
            let _guard = match cycle::cmp_enter(a.payload as usize, b.payload as usize) {
                Some(g) => g,
                None => return Ordering::Equal,
            };
            let v1 = c1.borrow();
            let v2 = c2.borrow();
            v1.cmp(&*v2)
        }

        // LibHandle — by u32 ID
        (HeapObject::LibHandle(h1), HeapObject::LibHandle(h2)) => h1.cmp(h2),

        // Fiber — stable identity via FiberHandle's Rc pointer.
        // Matches PartialEq; slot pointers are unstable across outbox
        // relocation on yield, so BTreeMap keyed on Fiber values would
        // lose entries if compared by slot address.
        (HeapObject::Fiber { handle: h1, .. }, HeapObject::Fiber { handle: h2, .. }) => {
            h1.id().cmp(&h2.id())
        }

        // ThreadHandle — stable identity via the Arc backing `result`.
        (
            HeapObject::ThreadHandle { handle: h1, .. },
            HeapObject::ThreadHandle { handle: h2, .. },
        ) => (std::sync::Arc::as_ptr(&h1.result) as usize)
            .cmp(&(std::sync::Arc::as_ptr(&h2.result) as usize)),

        // External — stable identity via the Rc backing `data`.
        (HeapObject::External { obj: o1, .. }, HeapObject::External { obj: o2, .. }) => {
            (std::rc::Rc::as_ptr(&o1.data) as *const () as usize)
                .cmp(&(std::rc::Rc::as_ptr(&o2.data) as *const () as usize))
        }

        // LSet — element-wise lexicographic (BTreeSet iteration is sorted)
        (HeapObject::LSet { data: s1, .. }, HeapObject::LSet { data: s2, .. }) => {
            s1.iter().cmp(s2.iter())
        }

        // LSetMut — element-wise lexicographic (borrow)
        (HeapObject::LSetMut { data: s1, .. }, HeapObject::LSetMut { data: s2, .. }) => {
            let _guard = match cycle::cmp_enter(a.payload as usize, b.payload as usize) {
                Some(g) => g,
                None => return Ordering::Equal,
            };
            let b1 = s1.borrow();
            let b2 = s2.borrow();
            b1.iter().cmp(b2.iter())
        }

        // All reference-identity types — by raw pointer (payload)
        _ => a.payload.cmp(&b.payload),
    }
}
// Debug is implemented in display.rs alongside Display, since both
// share the resolve_name helper for symbol/keyword resolution.
