//! Trait implementations for Value (PartialEq, Eq, Hash).
//!
//! The `traits` field on heap variants is NOT compared by PartialEq, NOT
//! hashed by Hash, and NOT compared by Ord. Trait identity is a separate
//! concern checked via `identical?`.

use std::hash::{Hash, Hasher};

use super::Value;

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        use crate::value::heap::{deref, HeapObject};

        // For immediate values, compare bits directly.
        //
        // Keywords store a 47-bit FNV-1a hash in the payload (via TAG_PTRVAL).
        // Same name → same hash → same bits. This is correct within a single
        // DSO and across DSO boundaries (all DSOs share the global keyword table).
        if !self.is_heap() && !other.is_heap() {
            return self.0 == other.0;
        }

        // If one is heap and the other isn't, they're not equal
        if self.is_heap() != other.is_heap() {
            return false;
        }

        // Both are heap values - dereference and compare contents
        unsafe {
            let self_obj = deref(*self);
            let other_obj = deref(*other);

            match (self_obj, other_obj) {
                // String comparison
                (HeapObject::LString { s: s1, .. }, HeapObject::LString { s: s2, .. }) => s1 == s2,

                // Cons cell comparison (Cons::PartialEq ignores traits)
                (HeapObject::Cons(c1), HeapObject::Cons(c2)) => c1 == c2,

                // Array comparison (compare contents)
                (
                    HeapObject::LArrayMut { data: v1, .. },
                    HeapObject::LArrayMut { data: v2, .. },
                ) => v1.borrow().as_slice() == v2.borrow().as_slice(),

                // Table comparison (compare contents)
                (
                    HeapObject::LStructMut { data: t1, .. },
                    HeapObject::LStructMut { data: t2, .. },
                ) => *t1.borrow() == *t2.borrow(),

                // Struct comparison (compare contents)
                (HeapObject::LStruct { data: s1, .. }, HeapObject::LStruct { data: s2, .. }) => {
                    s1 == s2
                }

                // Closure comparison (compare by reference)
                (
                    HeapObject::Closure { closure: c1, .. },
                    HeapObject::Closure { closure: c2, .. },
                ) => std::rc::Rc::ptr_eq(c1, c2),

                // Array comparison (compare contents element-wise)
                (
                    HeapObject::LArray { elements: t1, .. },
                    HeapObject::LArray { elements: t2, .. },
                ) => t1 == t2,

                // @string comparison (compare contents)
                (
                    HeapObject::LStringMut { data: b1, .. },
                    HeapObject::LStringMut { data: b2, .. },
                ) => *b1.borrow() == *b2.borrow(),

                // Box comparison (compare contents)
                (HeapObject::LBox { cell: c1, .. }, HeapObject::LBox { cell: c2, .. }) => {
                    *c1.borrow() == *c2.borrow()
                }

                // Float comparison — bitwise, not IEEE, so NaN == NaN (same bits)
                (HeapObject::Float(f1), HeapObject::Float(f2)) => f1.to_bits() == f2.to_bits(),

                // NativeFn comparison (compare by reference)
                (HeapObject::NativeFn(_), HeapObject::NativeFn(_)) => {
                    std::ptr::eq(self_obj as *const _, other_obj as *const _)
                }

                // LibHandle comparison
                (HeapObject::LibHandle(h1), HeapObject::LibHandle(h2)) => h1 == h2,

                // ThreadHandle comparison (compare by reference)
                (HeapObject::ThreadHandle { .. }, HeapObject::ThreadHandle { .. }) => {
                    std::ptr::eq(self_obj as *const _, other_obj as *const _)
                }

                // Fiber comparison (compare by reference)
                (HeapObject::Fiber { .. }, HeapObject::Fiber { .. }) => {
                    std::ptr::eq(self_obj as *const _, other_obj as *const _)
                }

                // Syntax comparison (by reference — same Rc)
                (HeapObject::Syntax { syntax: s1, .. }, HeapObject::Syntax { syntax: s2, .. }) => {
                    std::rc::Rc::ptr_eq(s1, s2)
                }

                // Binding comparison (by reference — same heap allocation)
                (HeapObject::Binding(_), HeapObject::Binding(_)) => {
                    std::ptr::eq(self_obj as *const _, other_obj as *const _)
                }

                // FFI signature comparison (structural equality, skip CIF cache)
                (HeapObject::FFISignature(s1, _), HeapObject::FFISignature(s2, _)) => s1 == s2,

                // FFI type descriptor comparison (structural equality)
                (HeapObject::FFIType(t1), HeapObject::FFIType(t2)) => t1 == t2,

                // Managed pointer comparison (by identity, not address)
                (HeapObject::ManagedPointer { .. }, HeapObject::ManagedPointer { .. }) => {
                    std::ptr::eq(self_obj as *const _, other_obj as *const _)
                }

                // External object comparison (by identity — same heap object)
                (HeapObject::External { .. }, HeapObject::External { .. }) => {
                    std::ptr::eq(self_obj as *const _, other_obj as *const _)
                }

                // Parameter comparison (by identity — same heap object)
                (HeapObject::Parameter { .. }, HeapObject::Parameter { .. }) => {
                    std::ptr::eq(self_obj as *const _, other_obj as *const _)
                }

                // Bytes comparison (compare contents)
                (HeapObject::LBytes { data: b1, .. }, HeapObject::LBytes { data: b2, .. }) => {
                    b1 == b2
                }

                // @bytes comparison (compare contents)
                (
                    HeapObject::LBytesMut { data: b1, .. },
                    HeapObject::LBytesMut { data: b2, .. },
                ) => *b1.borrow() == *b2.borrow(),

                // Set comparison (compare contents)
                (HeapObject::LSet { data: s1, .. }, HeapObject::LSet { data: s2, .. }) => s1 == s2,

                // Mutable set comparison (compare contents)
                (HeapObject::LSetMut { data: s1, .. }, HeapObject::LSetMut { data: s2, .. }) => {
                    *s1.borrow() == *s2.borrow()
                }

                // Different types are not equal
                _ => false,
            }
        }
    }
}

// NOTE: PartialEq is reflexive for all Value variants:
// - Immediate values: compared by raw bits (always reflexive)
// - Heap structural types: compared by contents (reflexive by induction)
// - Heap identity types: compared by pointer (always reflexive)
// - HeapObject::Float: compared by f64::to_bits() (always reflexive)
//
// The f64::to_bits() comparison means NaN == NaN (same bit pattern),
// which violates IEEE 754 but satisfies Eq's reflexivity requirement.
// This is intentional — set membership requires reflexivity.
impl Eq for Value {}

impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        use crate::value::heap::{deref, HeapObject};

        if !self.is_heap() {
            // Immediate values: raw bits encode the type tag + payload.
            // Same bits ↔ same value, and PartialEq agrees.
            //
            // SSO strings: same content → same bits → same hash.
            // Keywords: same name → same 47-bit FNV-1a hash → same bits → same hash.
            // Inline floats: same float bits → same Value bits.
            // TAG_NAN floats: NaN/Infinity encoded deterministically.
            self.0.hash(state);
            return;
        }

        unsafe {
            let obj = deref(*self);
            let tag = obj.tag();
            tag.hash(state);

            match obj {
                // Structural content types (immutable)
                HeapObject::LString { s, .. } => s.hash(state),
                // Cons::hash ignores traits field
                HeapObject::Cons(c) => c.hash(state),
                HeapObject::LArray { elements, .. } => elements.hash(state),
                HeapObject::LBytes { data, .. } => data.hash(state),
                HeapObject::LStruct { data: map, .. } => {
                    for (k, v) in map {
                        k.hash(state);
                        v.hash(state);
                    }
                }

                // Structural content types (mutable — hash current contents)
                HeapObject::LArrayMut { data: rc, .. } => {
                    let borrowed = rc.borrow();
                    borrowed.len().hash(state);
                    for v in borrowed.iter() {
                        v.hash(state);
                    }
                }
                HeapObject::LStructMut { data: rc, .. } => {
                    let borrowed = rc.borrow();
                    borrowed.len().hash(state);
                    for (k, v) in borrowed.iter() {
                        k.hash(state);
                        v.hash(state);
                    }
                }
                HeapObject::LStringMut { data: rc, .. } => rc.borrow().hash(state),
                HeapObject::LBytesMut { data: rc, .. } => rc.borrow().hash(state),
                HeapObject::LBox { cell: rc, .. } => rc.borrow().hash(state),

                // Structural-but-special heap types
                HeapObject::Float(f) => f.to_bits().hash(state),
                HeapObject::LibHandle(id) => id.hash(state),
                HeapObject::FFISignature(sig, _) => sig.hash(state),
                HeapObject::FFIType(desc) => desc.hash(state),

                // Reference-identity types: hash by raw Value bits (encodes pointer).
                // This matches PartialEq which uses pointer identity for these.
                _ => self.0.hash(state),
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

        // Fast path: identical bits → Equal
        if self.0 == other.0 {
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
    } else if v.is_int() {
        2
    } else if v.is_float() && !v.is_heap() {
        // Inline float (regular IEEE bits) or TAG_NAN encoded
        3
    } else if v.is_symbol() {
        4
    } else if v.is_keyword() {
        5
    } else if v.is_pointer() {
        6
    } else if v.is_empty_list() {
        7
    } else if (v.0 & super::TAG_SSO_MASK) == super::TAG_SSO {
        // SSO string — same rank as heap string
        8
    } else if v.is_heap() {
        match unsafe { deref(*v).tag() } {
            HeapTag::LString => 8, // same rank as SSO
            HeapTag::Float => 3,   // same rank as inline float
            HeapTag::Cons => 9,
            HeapTag::LArray => 10,
            HeapTag::LArrayMut => 11,
            HeapTag::LBytes => 12,
            HeapTag::LStringMut => 13,
            HeapTag::LBytesMut => 14,
            HeapTag::LStruct => 15,
            HeapTag::LStructMut => 16,
            HeapTag::Closure => 17,
            HeapTag::LBox => 18,
            HeapTag::NativeFn => 19,
            HeapTag::LibHandle => 20,
            HeapTag::ThreadHandle => 21,
            HeapTag::Fiber => 22,
            HeapTag::Syntax => 23,
            HeapTag::Binding => 24,
            HeapTag::FFISignature => 25,
            HeapTag::FFIType => 26,
            HeapTag::ManagedPointer => 27,
            HeapTag::External => 28,
            HeapTag::Parameter => 29,
            HeapTag::LSet => 30,
            HeapTag::LSetMut => 31,
        }
    } else {
        // Unknown — should not happen
        32
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

        // Int — numeric
        2 => {
            let a_int = a.as_int().unwrap();
            let b_int = b.as_int().unwrap();
            a_int.cmp(&b_int)
        }

        // Float (inline + heap) — f64::total_cmp
        3 => {
            let a_f = a.as_float().unwrap();
            let b_f = b.as_float().unwrap();
            a_f.total_cmp(&b_f)
        }

        // Symbol — by ID
        4 => {
            let a_id = a.as_symbol().unwrap();
            let b_id = b.as_symbol().unwrap();
            a_id.cmp(&b_id)
        }

        // Keyword — lexicographic by name
        5 => {
            let a_name = a.as_keyword_name().unwrap();
            let b_name = b.as_keyword_name().unwrap();
            a_name.cmp(&b_name)
        }

        // C pointer — by address bits
        6 => a.0.cmp(&b.0),

        // Empty list — singleton
        7 => Ordering::Equal,

        // String (SSO + heap) — lexicographic by content
        8 => a.compare_str(b).unwrap_or(Ordering::Equal),

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

    let a_obj = deref(*a);
    let b_obj = deref(*b);

    match (a_obj, b_obj) {
        // Cons — (first, rest) lexicographic (Cons::cmp ignores traits)
        (HeapObject::Cons(c1), HeapObject::Cons(c2)) => c1.cmp(c2),

        // Array — element-wise lexicographic
        (HeapObject::LArray { elements: t1, .. }, HeapObject::LArray { elements: t2, .. }) => {
            t1.cmp(t2)
        }

        // Array — element-wise lexicographic (borrow)
        (HeapObject::LArrayMut { data: a1, .. }, HeapObject::LArrayMut { data: a2, .. }) => {
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

        // Box — by contained value (borrow)
        (HeapObject::LBox { cell: c1, .. }, HeapObject::LBox { cell: c2, .. }) => {
            let v1 = c1.borrow();
            let v2 = c2.borrow();
            v1.cmp(&*v2)
        }

        // LibHandle — by u32 ID
        (HeapObject::LibHandle(h1), HeapObject::LibHandle(h2)) => h1.cmp(h2),

        // LSet — element-wise lexicographic (BTreeSet iteration is sorted)
        (HeapObject::LSet { data: s1, .. }, HeapObject::LSet { data: s2, .. }) => {
            s1.iter().cmp(s2.iter())
        }

        // LSetMut — element-wise lexicographic (borrow)
        (HeapObject::LSetMut { data: s1, .. }, HeapObject::LSetMut { data: s2, .. }) => {
            let b1 = s1.borrow();
            let b2 = s2.borrow();
            b1.iter().cmp(b2.iter())
        }

        // All reference-identity types — by raw pointer bits
        _ => a.0.cmp(&b.0),
    }
}
// Debug is implemented in display.rs alongside Display, since both
// share the resolve_name helper for symbol/keyword resolution.
