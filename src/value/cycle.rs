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
//! Pair cells use Floyd's tortoise-and-hare algorithm (O(1) space)
//! for cycle detection during list traversal.  This is belt-and-
//! suspenders: cons cells are immutable and cannot normally form
//! cycles, but the check costs almost nothing and defends against
//! future invariant violations.

use std::cell::RefCell;
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Cycle detection guards (macro-generated)
// ---------------------------------------------------------------------------

/// Generate a cycle-detection guard with a thread-local visited set.
///
/// Each guard is an RAII type that inserts a key on creation and removes
/// it on drop. The `enter` function returns `Some(guard)` on first visit
/// and `None` on reentry (cycle detected).
macro_rules! define_cycle_guard {
    ($vis:vis, $Guard:ident, $tls:ident, $enter:ident, $Key:ty) => {
        thread_local! {
            static $tls: RefCell<HashSet<$Key>> = RefCell::new(HashSet::new());
        }

        pub struct $Guard($Key);

        impl Drop for $Guard {
            fn drop(&mut self) {
                $tls.with(|v| {
                    v.borrow_mut().remove(&self.0);
                });
            }
        }

        #[inline]
        $vis fn $enter(key: $Key) -> Option<$Guard> {
            $tls.with(|v| {
                if v.borrow_mut().insert(key) {
                    Some($Guard(key))
                } else {
                    None
                }
            })
        }
    };
}

define_cycle_guard!(pub, FmtGuard, FMT_VISITED, fmt_enter, usize);
define_cycle_guard!(pub, HashGuard, HASH_VISITED, hash_enter, usize);
define_cycle_guard!(, CmpGuard, CMP_VISITED, cmp_enter_raw, [usize; 2]);

// ---------------------------------------------------------------------------
// Comparison entry point (normalizes pointer pairs)
// ---------------------------------------------------------------------------

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
    cmp_enter_raw(normalize_pair(ptr_a, ptr_b))
}

// ---------------------------------------------------------------------------
// Pair-cell tortoise-and-hare helper
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
            if let Some(c) = self.slow.as_pair() {
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
