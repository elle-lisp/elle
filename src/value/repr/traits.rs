//! Trait implementations for Value (PartialEq, Eq, Hash, Ord).
//!
//! The `traits` field on heap variants is NOT compared by PartialEq, NOT
//! hashed by Hash, and NOT compared by Ord. Trait identity is a separate
//! concern checked via `identical?`.

use std::hash::{Hash, Hasher};

use super::Value;
use crate::value::cycle;

mod ord;

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        super::eq::eq_with(self, other, super::eq::Relation::Identity)
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

                // Wrapper types: hash the backing Rc/Arc pointer, NOT the
                // slot address, because `with-traits` mints a second slot
                // over the same handle (see `repr/eq.rs`, "Wrapper variants
                // take their identity from the handle"). Hashing the slot
                // would put the two wrappers in different buckets.
                // Keep these in sync with the PartialEq arms in eq.rs.
                HeapObject::Fiber { handle, .. } => handle.id().hash(state),
                HeapObject::ThreadHandle { handle, .. } => {
                    (std::sync::Arc::as_ptr(&handle.result) as usize).hash(state)
                }
                HeapObject::External { obj, .. } => {
                    (std::rc::Rc::as_ptr(&obj.data) as *const () as usize).hash(state)
                }

                // Remaining reference-identity types (Closure,
                // ManagedPointer, Parameter, Syntax): hash by payload (the
                // slot pointer). These wrap no shared handle — their
                // `with-traits` copy duplicates the data outright, so it is
                // a genuinely distinct entity and the slot IS the identity.
                // (Native-fn is an immediate whose payload is its prim id,
                // so it never reaches this arm with an address.)
                _ => self.payload.hash(state),
            }
        }
    }
}
