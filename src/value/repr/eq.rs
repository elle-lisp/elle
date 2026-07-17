//! Structural equality, parameterized by relation.
//!
//! One traversal serves the two equality relations of docs/types.md
//! § Equality, so they can never disagree about structure — only about
//! number leaves:
//!
//! - `Identity` — Rust `PartialEq`, exposed to users as `identical?`:
//!   no numeric coercion, floats compare by bit pattern (NaN is
//!   reflexive, -0.0 ≠ 0.0).
//! - `Numeric` — the language `=`: int/int exact, mixed int/float
//!   promotes to f64, floats follow IEEE 754 (NaN ≠ NaN, -0.0 = 0.0),
//!   at every depth. Compositional: `(= [a] [b])` ⇔ `(= a b)`. There is
//!   no pointer-identity shortcut — a value containing NaN is not `=`
//!   to anything, including itself.
//!
//! Key equivalence (Value::Hash, Value::Ord, TableKey) is a third,
//! separate relation: number-coercive like `Numeric` but NaN-reflexive
//! like `Identity`, so NaN-holding values stay findable in sets.

use super::Value;
use crate::value::cycle;
use crate::value::heap::{deref, HeapObject, Pair};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Relation {
    /// Bit-pattern scalars; the strict relation behind `PartialEq` and
    /// `identical?`.
    Identity,
    /// The language `=`: coercive, IEEE 754, compositional.
    Numeric,
}

pub(crate) fn eq_with(a: &Value, b: &Value, rel: Relation) -> bool {
    // Immediates: compare tag+payload, except `=` compares numbers
    // numerically (int/int exact; mixed/float promotes to f64, IEEE).
    if !a.is_heap() && !b.is_heap() {
        if rel == Relation::Numeric && a.is_number() && b.is_number() {
            if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
                return x == y;
            }
            return match (a.as_number(), b.as_number()) {
                (Some(x), Some(y)) => x == y,
                _ => false,
            };
        }
        return a.tag == b.tag && a.payload == b.payload;
    }

    // If one is heap and the other isn't, they're not equal.
    if a.is_heap() != b.is_heap() {
        return false;
    }

    // Pointer-identity fast path is Identity-only: under `=`, a NaN
    // anywhere inside makes a value unequal even to itself.
    if rel == Relation::Identity && a.payload == b.payload {
        return true;
    }

    // Both are heap values — dereference and compare contents.
    // Mutable/immutable pairs of the same collection type are equal
    // if their contents are equal: (= [1 2] @[1 2]) is true.
    unsafe {
        let a_obj = deref(*a);
        let b_obj = deref(*b);

        match (a_obj, b_obj) {
            // String: immutable × immutable
            (HeapObject::LString { s: s1, .. }, HeapObject::LString { s: s2, .. }) => s1 == s2,
            // String × @string
            (HeapObject::LString { s: s1, .. }, HeapObject::LStringMut { data: b2, .. })
            | (HeapObject::LStringMut { data: b2, .. }, HeapObject::LString { s: s1, .. }) => {
                s1.as_slice() == b2.borrow().as_slice()
            }
            // @string × @string
            (HeapObject::LStringMut { data: b1, .. }, HeapObject::LStringMut { data: b2, .. }) => {
                *b1.borrow() == *b2.borrow()
            }

            // Pair cells: recurse on first, iterate on rest (traits
            // intentionally excluded, like Pair::PartialEq).
            (HeapObject::Pair(c1), HeapObject::Pair(c2)) => pair_eq(c1, c2, rel),

            // Array: immutable × immutable
            (HeapObject::LArray { elements: a1, .. }, HeapObject::LArray { elements: a2, .. }) => {
                slice_eq(a1.as_slice(), a2.as_slice(), rel)
            }
            // Array × @array
            (HeapObject::LArray { elements: a1, .. }, HeapObject::LArrayMut { data: a2, .. })
            | (HeapObject::LArrayMut { data: a2, .. }, HeapObject::LArray { elements: a1, .. }) => {
                slice_eq(a1.as_slice(), a2.borrow().as_slice(), rel)
            }
            // @array × @array
            (HeapObject::LArrayMut { data: v1, .. }, HeapObject::LArrayMut { data: v2, .. }) => {
                let _guard = match cycle::cmp_enter(a.payload as usize, b.payload as usize) {
                    Some(g) => g,
                    None => return true, // cycle: assume equal
                };
                slice_eq(v1.borrow().as_slice(), v2.borrow().as_slice(), rel)
            }

            // Struct: immutable × immutable (sorted Vec vs sorted Vec).
            // TableKey comparison is relation-independent: keys reject
            // floats, so the relations cannot disagree on them.
            (HeapObject::LStruct { data: s1, .. }, HeapObject::LStruct { data: s2, .. }) => {
                s1.len() == s2.len()
                    && s1
                        .iter()
                        .zip(s2.iter())
                        .all(|((k1, v1), (k2, v2))| k1 == k2 && eq_with(v1, v2, rel))
            }
            // Struct × @struct (sorted Vec vs BTreeMap)
            (HeapObject::LStruct { data: s1, .. }, HeapObject::LStructMut { data: s2, .. })
            | (HeapObject::LStructMut { data: s2, .. }, HeapObject::LStruct { data: s1, .. }) => {
                let borrowed = s2.borrow();
                s1.len() == borrowed.len()
                    && s1
                        .iter()
                        .all(|(k, v)| borrowed.get(k).is_some_and(|v2| eq_with(v, v2, rel)))
            }
            // @struct × @struct
            (HeapObject::LStructMut { data: t1, .. }, HeapObject::LStructMut { data: t2, .. }) => {
                let _guard = match cycle::cmp_enter(a.payload as usize, b.payload as usize) {
                    Some(g) => g,
                    None => return true,
                };
                let (m1, m2) = (t1.borrow(), t2.borrow());
                m1.len() == m2.len()
                    && m1
                        .iter()
                        .zip(m2.iter())
                        .all(|((k1, v1), (k2, v2))| k1 == k2 && eq_with(v1, v2, rel))
            }

            // Bytes: immutable × immutable
            (HeapObject::LBytes { data: b1, .. }, HeapObject::LBytes { data: b2, .. }) => b1 == b2,
            // Bytes × @bytes
            (HeapObject::LBytes { data: b1, .. }, HeapObject::LBytesMut { data: b2, .. })
            | (HeapObject::LBytesMut { data: b2, .. }, HeapObject::LBytes { data: b1, .. }) => {
                b1.as_slice() == b2.borrow().as_slice()
            }
            // @bytes × @bytes
            (HeapObject::LBytesMut { data: b1, .. }, HeapObject::LBytesMut { data: b2, .. }) => {
                *b1.borrow() == *b2.borrow()
            }

            // Set: immutable × immutable. Sets are Ord-sorted, and Ord
            // agrees with both relations on element order (coercively
            // equal numbers are Ord-equal, so they cannot be adjacent
            // out of order), so positional zip is sound.
            (HeapObject::LSet { data: s1, .. }, HeapObject::LSet { data: s2, .. }) => {
                slice_eq(s1.as_slice(), s2.as_slice(), rel)
            }
            // Set × @set
            (HeapObject::LSet { data: s1, .. }, HeapObject::LSetMut { data: s2, .. })
            | (HeapObject::LSetMut { data: s2, .. }, HeapObject::LSet { data: s1, .. }) => {
                let borrowed = s2.borrow();
                s1.len() == borrowed.len()
                    && s1
                        .iter()
                        .zip(borrowed.iter())
                        .all(|(x, y)| eq_with(x, y, rel))
            }
            // @set × @set
            (HeapObject::LSetMut { data: s1, .. }, HeapObject::LSetMut { data: s2, .. }) => {
                let _guard = match cycle::cmp_enter(a.payload as usize, b.payload as usize) {
                    Some(g) => g,
                    None => return true,
                };
                let (b1, b2) = (s1.borrow(), s2.borrow());
                b1.len() == b2.len() && b1.iter().zip(b2.iter()).all(|(x, y)| eq_with(x, y, rel))
            }

            // Closure comparison (compare by identity of the arena-resident
            // Closure: two closure Values are structurally equal iff they
            // point at the same HeapObject).
            (HeapObject::Closure { closure: c1, .. }, HeapObject::Closure { closure: c2, .. }) => {
                std::ptr::eq(c1, c2)
            }

            // Box comparison (compare contents)
            (HeapObject::LBox { cell: c1, .. }, HeapObject::LBox { cell: c2, .. })
            | (
                HeapObject::CaptureCell { cell: c1, .. },
                HeapObject::CaptureCell { cell: c2, .. },
            ) => {
                let _guard = match cycle::cmp_enter(a.payload as usize, b.payload as usize) {
                    Some(g) => g,
                    None => return true,
                };
                eq_with(&c1.borrow(), &c2.borrow(), rel)
            }

            // (Native-fns are immediates now — `Value{TAG_NATIVE_FN, prim_id}` —
            // and compare by prim_id on the immediate fast path above; they never
            // reach this heap-deref match.)

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
                std::ptr::eq(a_obj as *const _, b_obj as *const _)
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
                std::ptr::eq(a_obj as *const _, b_obj as *const _)
            }

            // Different types are not equal.
            _ => false,
        }
    }
}

/// Element-wise slice equality under `rel`.
fn slice_eq(a: &[Value], b: &[Value], rel: Relation) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| eq_with(x, y, rel))
}

/// Pair-chain equality: recurse on `first`, iterate on `rest` so long
/// proper lists don't recurse one Rust frame per element.
fn pair_eq(mut a: &Pair, mut b: &Pair, rel: Relation) -> bool {
    loop {
        if !eq_with(&a.first, &b.first, rel) {
            return false;
        }
        match (a.rest.as_pair(), b.rest.as_pair()) {
            (Some(p1), Some(p2)) => {
                a = p1;
                b = p2;
            }
            _ => return eq_with(&a.rest, &b.rest, rel),
        }
    }
}
