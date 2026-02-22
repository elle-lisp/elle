//! Trait implementations for Value (PartialEq, Debug, etc.)

use super::Value;

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        use crate::value::heap::{deref, HeapObject};

        // For immediate values, compare bits directly
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
                (HeapObject::String(s1), HeapObject::String(s2)) => s1 == s2,

                // Cons cell comparison
                (HeapObject::Cons(c1), HeapObject::Cons(c2)) => c1 == c2,

                // Vector comparison (compare contents)
                (HeapObject::Vector(v1), HeapObject::Vector(v2)) => {
                    v1.borrow().as_slice() == v2.borrow().as_slice()
                }

                // Table comparison (compare contents)
                (HeapObject::Table(t1), HeapObject::Table(t2)) => *t1.borrow() == *t2.borrow(),

                // Struct comparison (compare contents)
                (HeapObject::Struct(s1), HeapObject::Struct(s2)) => s1 == s2,

                // Closure comparison (compare by reference)
                (HeapObject::Closure(c1), HeapObject::Closure(c2)) => std::rc::Rc::ptr_eq(c1, c2),

                // Tuple comparison (compare contents element-wise)
                (HeapObject::Tuple(t1), HeapObject::Tuple(t2)) => t1 == t2,

                // Cell comparison (compare contents)
                (HeapObject::Cell(c1, _), HeapObject::Cell(c2, _)) => *c1.borrow() == *c2.borrow(),

                // Float comparison
                (HeapObject::Float(f1), HeapObject::Float(f2)) => f1 == f2,

                // NativeFn comparison (compare by reference)
                (HeapObject::NativeFn(_), HeapObject::NativeFn(_)) => {
                    // Function pointers are compared by reference (pointer equality)
                    // Since they're stored in an Rc, we compare the Rc pointers
                    std::ptr::eq(self_obj as *const _, other_obj as *const _)
                }

                // LibHandle comparison
                (HeapObject::LibHandle(h1), HeapObject::LibHandle(h2)) => h1 == h2,

                // CHandle comparison
                (HeapObject::CHandle(p1, h1), HeapObject::CHandle(p2, h2)) => p1 == p2 && h1 == h2,

                // ThreadHandle comparison (compare by reference)
                (HeapObject::ThreadHandle(_), HeapObject::ThreadHandle(_)) => {
                    std::ptr::eq(self_obj as *const _, other_obj as *const _)
                }

                // Fiber comparison (compare by reference)
                (HeapObject::Fiber(_), HeapObject::Fiber(_)) => {
                    std::ptr::eq(self_obj as *const _, other_obj as *const _)
                }

                // Different types are not equal
                _ => false,
            }
        }
    }
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Delegate to Display implementation
        write!(f, "{}", self)
    }
}
