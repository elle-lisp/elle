//! Trait implementations for Value (PartialEq, Eq, Hash, Ord).
//!
//! The `traits` field on heap variants is NOT compared by PartialEq, NOT
//! hashed by Hash, and NOT compared by Ord. Trait identity is a separate
//! concern checked via `identical?`.

use std::hash::{Hash, Hasher};

use super::Value;
use crate::value::cycle;

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        use crate::value::heap::{deref, HeapObject};

        // For immediate values, compare tag+payload directly.
        // Same tag and same payload means the same value.
        if !self.is_heap() && !other.is_heap() {
            return self.tag == other.tag && self.payload == other.payload;
        }

        // If one is heap and the other isn't, they're not equal.
        if self.is_heap() != other.is_heap() {
            return false;
        }

        // Pointer-identity fast path: same heap object → equal.
        if self.payload == other.payload {
            return true;
        }

        // Both are heap values — dereference and compare contents.
        // Mutable/immutable pairs of the same collection type are equal
        // if their contents are equal: (= [1 2] @[1 2]) is true.
        unsafe {
            let self_obj = deref(*self);
            let other_obj = deref(*other);

            match (self_obj, other_obj) {
                // String: immutable × immutable
                (HeapObject::LString { s: s1, .. }, HeapObject::LString { s: s2, .. }) => s1 == s2,
                // String × @string
                (HeapObject::LString { s: s1, .. }, HeapObject::LStringMut { data: b2, .. })
                | (HeapObject::LStringMut { data: b2, .. }, HeapObject::LString { s: s1, .. }) => {
                    s1.as_slice() == b2.borrow().as_slice()
                }
                // @string × @string
                (
                    HeapObject::LStringMut { data: b1, .. },
                    HeapObject::LStringMut { data: b2, .. },
                ) => *b1.borrow() == *b2.borrow(),

                // Pair cell comparison (Pair::PartialEq ignores traits)
                (HeapObject::Pair(c1), HeapObject::Pair(c2)) => c1 == c2,

                // Array: immutable × immutable
                (
                    HeapObject::LArray { elements: a1, .. },
                    HeapObject::LArray { elements: a2, .. },
                ) => a1 == a2,
                // Array × @array
                (
                    HeapObject::LArray { elements: a1, .. },
                    HeapObject::LArrayMut { data: a2, .. },
                )
                | (
                    HeapObject::LArrayMut { data: a2, .. },
                    HeapObject::LArray { elements: a1, .. },
                ) => a1.as_slice() == a2.borrow().as_slice(),
                // @array × @array
                (
                    HeapObject::LArrayMut { data: v1, .. },
                    HeapObject::LArrayMut { data: v2, .. },
                ) => {
                    let _guard =
                        match cycle::cmp_enter(self.payload as usize, other.payload as usize) {
                            Some(g) => g,
                            None => return true, // cycle: assume equal
                        };
                    v1.borrow().as_slice() == v2.borrow().as_slice()
                }

                // Struct: immutable × immutable (sorted Vec vs sorted Vec)
                (HeapObject::LStruct { data: s1, .. }, HeapObject::LStruct { data: s2, .. }) => {
                    s1 == s2
                }
                // Struct × @struct (sorted Vec vs BTreeMap)
                (HeapObject::LStruct { data: s1, .. }, HeapObject::LStructMut { data: s2, .. })
                | (HeapObject::LStructMut { data: s2, .. }, HeapObject::LStruct { data: s1, .. }) =>
                {
                    let borrowed = s2.borrow();
                    s1.len() == borrowed.len() && s1.iter().all(|(k, v)| borrowed.get(k) == Some(v))
                }
                // @struct × @struct
                (
                    HeapObject::LStructMut { data: t1, .. },
                    HeapObject::LStructMut { data: t2, .. },
                ) => {
                    let _guard =
                        match cycle::cmp_enter(self.payload as usize, other.payload as usize) {
                            Some(g) => g,
                            None => return true,
                        };
                    *t1.borrow() == *t2.borrow()
                }

                // Bytes: immutable × immutable
                (HeapObject::LBytes { data: b1, .. }, HeapObject::LBytes { data: b2, .. }) => {
                    b1 == b2
                }
                // Bytes × @bytes
                (HeapObject::LBytes { data: b1, .. }, HeapObject::LBytesMut { data: b2, .. })
                | (HeapObject::LBytesMut { data: b2, .. }, HeapObject::LBytes { data: b1, .. }) => {
                    b1.as_slice() == b2.borrow().as_slice()
                }
                // @bytes × @bytes
                (
                    HeapObject::LBytesMut { data: b1, .. },
                    HeapObject::LBytesMut { data: b2, .. },
                ) => *b1.borrow() == *b2.borrow(),

                // Set: immutable × immutable
                (HeapObject::LSet { data: s1, .. }, HeapObject::LSet { data: s2, .. }) => s1 == s2,
                // Set × @set
                (HeapObject::LSet { data: s1, .. }, HeapObject::LSetMut { data: s2, .. })
                | (HeapObject::LSetMut { data: s2, .. }, HeapObject::LSet { data: s1, .. }) => {
                    let borrowed = s2.borrow();
                    s1.len() == borrowed.len()
                        && s1.iter().zip(borrowed.iter()).all(|(a, b)| a == b)
                }
                // @set × @set
                (HeapObject::LSetMut { data: s1, .. }, HeapObject::LSetMut { data: s2, .. }) => {
                    let _guard =
                        match cycle::cmp_enter(self.payload as usize, other.payload as usize) {
                            Some(g) => g,
                            None => return true,
                        };
                    *s1.borrow() == *s2.borrow()
                }

                // Closure comparison (compare by identity of the arena-resident
                // Closure: two closure Values are structurally equal iff they
                // point at the same HeapObject).
                (
                    HeapObject::Closure { closure: c1, .. },
                    HeapObject::Closure { closure: c2, .. },
                ) => std::ptr::eq(c1, c2),

                // Box comparison (compare contents)
                (HeapObject::LBox { cell: c1, .. }, HeapObject::LBox { cell: c2, .. })
                | (
                    HeapObject::CaptureCell { cell: c1, .. },
                    HeapObject::CaptureCell { cell: c2, .. },
                ) => {
                    let _guard =
                        match cycle::cmp_enter(self.payload as usize, other.payload as usize) {
                            Some(g) => g,
                            None => return true,
                        };
                    *c1.borrow() == *c2.borrow()
                }

                // NativeFn comparison (compare by reference)
                (HeapObject::NativeFn(_), HeapObject::NativeFn(_)) => {
                    std::ptr::eq(self_obj as *const _, other_obj as *const _)
                }

                // LibHandle comparison
                (HeapObject::LibHandle(h1), HeapObject::LibHandle(h2)) => h1 == h2,

                // ThreadHandle comparison: stable identity via the `Arc`
                // backing `result`. Comparing slot pointers would break
                // when a ThreadHandle value is relocated (e.g., copied to
                // another fiber's outbox on yield) — the same underlying
                // handle would then become a distinct map key.
                (
                    HeapObject::ThreadHandle { handle: h1, .. },
                    HeapObject::ThreadHandle { handle: h2, .. },
                ) => std::sync::Arc::ptr_eq(&h1.result, &h2.result),

                // Fiber comparison: stable identity via the `Rc` inside
                // the `FiberHandle`. Slot-pointer equality is wrong here
                // because `deep_copy_to_outbox` re-allocates the Fiber
                // slot on yield; both slots wrap clones of the same
                // handle and must be treated as the same fiber so that
                // scheduler maps keyed on fibers (`waiters`, `completed`)
                // don't desync.
                (HeapObject::Fiber { handle: h1, .. }, HeapObject::Fiber { handle: h2, .. }) => {
                    h1.id() == h2.id()
                }

                // Syntax comparison (by reference — same Box)
                (HeapObject::Syntax { syntax: s1, .. }, HeapObject::Syntax { syntax: s2, .. }) => {
                    std::ptr::eq(&**s1, &**s2)
                }

                // FFI signature comparison (structural equality, skip CIF cache)
                (HeapObject::FFISignature(s1, _), HeapObject::FFISignature(s2, _)) => s1 == s2,

                // FFI type descriptor comparison (structural equality)
                (HeapObject::FFIType(t1), HeapObject::FFIType(t2)) => t1 == t2,

                // Managed pointer comparison (by identity, not address)
                (HeapObject::ManagedPointer { .. }, HeapObject::ManagedPointer { .. }) => {
                    std::ptr::eq(self_obj as *const _, other_obj as *const _)
                }

                // External object comparison: stable identity via the
                // `Rc<dyn Any>` backing `data`. See Fiber/ThreadHandle
                // rationale — slot pointers are unstable across outbox
                // relocation.
                (HeapObject::External { obj: o1, .. }, HeapObject::External { obj: o2, .. }) => {
                    std::rc::Rc::ptr_eq(&o1.data, &o2.data)
                }

                // Parameter comparison (by identity — same heap object)
                (HeapObject::Parameter { .. }, HeapObject::Parameter { .. }) => {
                    std::ptr::eq(self_obj as *const _, other_obj as *const _)
                }

                // Different types are not equal.
                _ => false,
            }
        }
    }
}

// NOTE: PartialEq is reflexive for all Value variants:
// - Immediate values: compared by tag+payload (always reflexive)
// - Heap structural types: compared by contents (reflexive by induction)
// - Heap identity types: compared by pointer (always reflexive)
//
// Float NaN == NaN (same bit pattern) since payload is f64::to_bits(),
// which violates IEEE 754 but satisfies Eq's reflexivity requirement.
// This is intentional — set membership requires reflexivity.
impl Eq for Value {}

impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        use crate::value::heap::{deref, HeapObject};

        if !self.is_heap() {
            // Numeric coercion: (= 1 1.0) is true, so they must hash
            // identically.  Canonicalize all numbers to their f64 bits.
            if let Some(f) = self.as_number() {
                // Use a fixed discriminator so int 1 and float 1.0 match.
                0xFFu8.hash(state);
                f.to_bits().hash(state);
                return;
            }
            // Non-numeric immediates: tag + payload is unique and matches PartialEq.
            self.tag.hash(state);
            self.payload.hash(state);
            return;
        }

        unsafe {
            let obj = deref(*self);
            let tag = obj.tag();
            tag.hash(state);

            match obj {
                // Structural content types (immutable)
                HeapObject::LString { s, .. } => s.hash(state),
                // Pair::hash ignores traits field
                HeapObject::Pair(c) => c.hash(state),
                HeapObject::LArray { elements, .. } => elements.hash(state),
                HeapObject::LBytes { data, .. } => data.hash(state),
                HeapObject::LStruct { data: entries, .. } => {
                    for (k, v) in entries {
                        k.hash(state);
                        v.hash(state);
                    }
                }

                // Structural content types (mutable — hash current contents)
                // Cycle detection: on re-entry, hash nothing more (the tag
                // was already hashed above, giving a stable sentinel).
                HeapObject::LArrayMut { data: rc, .. } => {
                    if let Some(_guard) = cycle::hash_enter(self.payload as usize) {
                        let borrowed = rc.borrow();
                        borrowed.len().hash(state);
                        for v in borrowed.iter() {
                            v.hash(state);
                        }
                    }
                }
                HeapObject::LStructMut { data: rc, .. } => {
                    if let Some(_guard) = cycle::hash_enter(self.payload as usize) {
                        let borrowed = rc.borrow();
                        borrowed.len().hash(state);
                        for (k, v) in borrowed.iter() {
                            k.hash(state);
                            v.hash(state);
                        }
                    }
                }
                HeapObject::LStringMut { data: rc, .. } => rc.borrow().hash(state),
                HeapObject::LBytesMut { data: rc, .. } => rc.borrow().hash(state),
                HeapObject::LBox { cell: rc, .. } | HeapObject::CaptureCell { cell: rc, .. } => {
                    if let Some(_guard) = cycle::hash_enter(self.payload as usize) {
                        rc.borrow().hash(state);
                    }
                }
                HeapObject::LSetMut { data: rc, .. } => {
                    if let Some(_guard) = cycle::hash_enter(self.payload as usize) {
                        let borrowed = rc.borrow();
                        borrowed.len().hash(state);
                        for v in borrowed.iter() {
                            v.hash(state);
                        }
                    }
                }

                // Structural-but-special heap types
                HeapObject::LibHandle(id) => id.hash(state),
                HeapObject::FFISignature(sig, _) => sig.hash(state),
                HeapObject::FFIType(desc) => desc.hash(state),

                // Stable-identity types: hash by the backing Rc/Arc
                // pointer, NOT by the slot address. Slot pointers are
                // unstable under outbox relocation on fiber yield;
                // hashing them would break map lookups across yields.
                // Keep these in sync with the PartialEq arms above.
                HeapObject::Fiber { handle, .. } => handle.id().hash(state),
                HeapObject::ThreadHandle { handle, .. } => {
                    (std::sync::Arc::as_ptr(&handle.result) as usize).hash(state)
                }
                HeapObject::External { obj, .. } => {
                    (std::rc::Rc::as_ptr(&obj.data) as *const () as usize).hash(state)
                }

                // Remaining reference-identity types (Closure,
                // NativeFn, ManagedPointer, Parameter, Syntax): hash by
                // payload (the slot pointer). These are not subject to
                // outbox relocation under the current model.
                _ => self.payload.hash(state),
            }
        }
    }
}

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
            HeapTag::NativeFn => 18,
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
