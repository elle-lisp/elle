//! Cycle detection for recursive value traversal.
//!
//! Mutable containers (`@[]`, `@{}`, `@||`, `LBox`) can form reference
//! cycles via mutation.  The `Display`, `Hash`, `PartialEq`, and `Ord`
//! trait impls recurse into contained values, which causes a stack
//! overflow on cyclic structures.
//!
//! This module provides three independent thread-local visited sets —
//! one for formatting, one for hashing, and one for comparison — with
//! RAII guards that automatically clean up on scope exit (including
//! panics and early returns).
//!
//! Cons cells use Floyd's tortoise-and-hare algorithm (O(1) space)
//! for cycle detection during list traversal.  This is belt-and-
//! suspenders: cons cells are immutable and cannot normally form
//! cycles, but the check costs almost nothing and defends against
//! future invariant violations.

use std::cell::RefCell;
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Display / Debug cycle detection (single-pointer visited set)
// ---------------------------------------------------------------------------

thread_local! {
    static FMT_VISITED: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

/// RAII guard that removes a pointer from the formatting visited set on drop.
pub struct FmtGuard(usize);

impl Drop for FmtGuard {
    fn drop(&mut self) {
        FMT_VISITED.with(|v| {
            v.borrow_mut().remove(&self.0);
        });
    }
}

/// Try to enter a mutable container for formatting.
///
/// Returns `Some(guard)` on first visit (proceed with formatting).
/// Returns `None` if already being formatted (cycle detected).
/// The guard removes the entry automatically on drop.
#[inline]
pub fn fmt_enter(heap_ptr: usize) -> Option<FmtGuard> {
    FMT_VISITED.with(|v| {
        if v.borrow_mut().insert(heap_ptr) {
            Some(FmtGuard(heap_ptr))
        } else {
            None
        }
    })
}

// ---------------------------------------------------------------------------
// Hash cycle detection (single-pointer visited set)
// ---------------------------------------------------------------------------

thread_local! {
    static HASH_VISITED: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

/// RAII guard that removes a pointer from the hashing visited set on drop.
pub struct HashGuard(usize);

impl Drop for HashGuard {
    fn drop(&mut self) {
        HASH_VISITED.with(|v| {
            v.borrow_mut().remove(&self.0);
        });
    }
}

/// Try to enter a mutable container for hashing.
///
/// Returns `Some(guard)` on first visit (proceed with hashing).
/// Returns `None` if already being hashed (cycle — hash nothing more).
#[inline]
pub fn hash_enter(heap_ptr: usize) -> Option<HashGuard> {
    HASH_VISITED.with(|v| {
        if v.borrow_mut().insert(heap_ptr) {
            Some(HashGuard(heap_ptr))
        } else {
            None
        }
    })
}

// ---------------------------------------------------------------------------
// PartialEq / Ord cycle detection (pointer-pair visited set)
// ---------------------------------------------------------------------------

thread_local! {
    static CMP_VISITED: RefCell<HashSet<[usize; 2]>> = RefCell::new(HashSet::new());
}

/// RAII guard that removes a pointer pair from the comparison visited set.
pub struct CmpGuard([usize; 2]);

impl Drop for CmpGuard {
    fn drop(&mut self) {
        CMP_VISITED.with(|v| {
            v.borrow_mut().remove(&self.0);
        });
    }
}

/// Normalize a pair so (a, b) and (b, a) map to the same key.
#[inline]
fn normalize_pair(a: usize, b: usize) -> [usize; 2] {
    if a <= b {
        [a, b]
    } else {
        [b, a]
    }
}

/// Try to enter a mutable-container pair for comparison (PartialEq / Ord).
///
/// Returns `Some(guard)` on first visit (proceed with comparison).
/// Returns `None` if this pair is already being compared (cycle —
/// assume equal / return `Ordering::Equal`).
#[inline]
pub fn cmp_enter(ptr_a: usize, ptr_b: usize) -> Option<CmpGuard> {
    let key = normalize_pair(ptr_a, ptr_b);
    CMP_VISITED.with(|v| {
        if v.borrow_mut().insert(key) {
            Some(CmpGuard(key))
        } else {
            None
        }
    })
}

// ---------------------------------------------------------------------------
// Cons-cell tortoise-and-hare helper
// ---------------------------------------------------------------------------

use super::Value;

/// State for Floyd's cycle detection during cons-cell traversal.
///
/// Call `advance()` for each cdr step.  When it returns `true` the
/// current cell has been seen before — stop and report a cycle.
pub struct HareState {
    slow: Value,
    fast: Value,
    step: usize,
}

impl HareState {
    /// Create a new hare state anchored at the head of the list.
    #[inline]
    pub fn new(head: Value) -> Self {
        HareState {
            slow: head,
            fast: head,
            step: 0,
        }
    }

    /// Advance one cdr step.  Returns `true` if a cycle is detected.
    ///
    /// `current` is the cons cell we just moved to (the new value of
    /// the "current" pointer in the display loop).
    #[inline]
    pub fn advance(&mut self, current: Value) -> bool {
        self.step += 1;

        // Tortoise moves every other step
        if self.step.is_multiple_of(2) {
            if let Some(c) = self.slow.as_cons() {
                self.slow = c.rest;
            }
        }

        // Hare moves every step (by tracking `current`)
        self.fast = current;

        // Compare by heap pointer identity
        if self.step >= 2 {
            if let (Some(s), Some(f)) = (self.slow.as_heap_ptr(), self.fast.as_heap_ptr()) {
                return std::ptr::eq(s, f);
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_enter_detects_reentry() {
        let ptr = 0xDEAD_BEEF_usize;
        let guard = fmt_enter(ptr);
        assert!(guard.is_some(), "first entry should succeed");
        assert!(fmt_enter(ptr).is_none(), "reentry should fail");
        drop(guard);
        let guard2 = fmt_enter(ptr);
        assert!(guard2.is_some(), "entry after drop should succeed");
    }

    #[test]
    fn hash_enter_detects_reentry() {
        let ptr = 0xCAFE_BABE_usize;
        let guard = hash_enter(ptr);
        assert!(guard.is_some());
        assert!(hash_enter(ptr).is_none());
        drop(guard);
        assert!(hash_enter(ptr).is_some());
    }

    #[test]
    fn cmp_enter_normalizes_pair_order() {
        let a = 100_usize;
        let b = 200_usize;
        let guard = cmp_enter(a, b);
        assert!(guard.is_some());
        // (b, a) should hit the same entry
        assert!(cmp_enter(b, a).is_none());
        // (a, b) should also hit
        assert!(cmp_enter(a, b).is_none());
        drop(guard);
        assert!(cmp_enter(a, b).is_some());
    }

    #[test]
    fn cmp_enter_same_pointer_twice() {
        let a = 42_usize;
        let guard = cmp_enter(a, a);
        assert!(guard.is_some());
        assert!(cmp_enter(a, a).is_none());
        drop(guard);
    }
}
