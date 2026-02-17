//! Continuation arena for efficient allocation
//!
//! Continuations are allocated frequently during CPS execution.
//! Using an arena reduces allocation overhead and improves cache locality.

use super::Continuation;
use crate::compiler::ast::Expr;
use std::cell::RefCell;
use std::rc::Rc;

/// Arena allocator for continuations
///
/// Continuations are allocated in batches and freed together,
/// which is efficient for the typical coroutine execution pattern.
pub struct ContinuationArena {
    /// Pool of allocated continuations
    pool: RefCell<Vec<Rc<Continuation>>>,
    /// Statistics
    stats: RefCell<ArenaStats>,
}

/// Statistics about arena usage
#[derive(Debug, Clone, Default)]
pub struct ArenaStats {
    /// Total allocations
    pub allocations: usize,
    /// Current pool size
    pub pool_size: usize,
    /// Peak pool size
    pub peak_size: usize,
}

impl ContinuationArena {
    /// Create a new arena
    pub fn new() -> Self {
        Self {
            pool: RefCell::new(Vec::with_capacity(64)),
            stats: RefCell::new(ArenaStats::default()),
        }
    }

    /// Create an arena with pre-allocated capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            pool: RefCell::new(Vec::with_capacity(capacity)),
            stats: RefCell::new(ArenaStats::default()),
        }
    }

    /// Allocate a continuation in the arena
    pub fn alloc(&self, cont: Continuation) -> Rc<Continuation> {
        let rc = Rc::new(cont);

        let mut pool = self.pool.borrow_mut();
        let mut stats = self.stats.borrow_mut();

        pool.push(rc.clone());
        stats.allocations += 1;
        stats.pool_size = pool.len();
        if stats.pool_size > stats.peak_size {
            stats.peak_size = stats.pool_size;
        }

        rc
    }

    /// Allocate a Done continuation
    pub fn done(&self) -> Rc<Continuation> {
        self.alloc(Continuation::Done)
    }

    /// Allocate a Sequence continuation
    pub fn sequence(&self, remaining: Vec<Expr>, next: Rc<Continuation>) -> Rc<Continuation> {
        if remaining.is_empty() {
            next
        } else {
            self.alloc(Continuation::Sequence { remaining, next })
        }
    }

    /// Clear the arena, releasing all continuations
    ///
    /// This should be called after a coroutine completes or is abandoned.
    pub fn clear(&self) {
        self.pool.borrow_mut().clear();
        self.stats.borrow_mut().pool_size = 0;
    }

    /// Get arena statistics
    pub fn stats(&self) -> ArenaStats {
        self.stats.borrow().clone()
    }

    /// Get current pool size
    pub fn len(&self) -> usize {
        self.pool.borrow().len()
    }

    /// Check if arena is empty
    pub fn is_empty(&self) -> bool {
        self.pool.borrow().is_empty()
    }
}

impl Default for ContinuationArena {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    #[test]
    fn test_arena_alloc() {
        let arena = ContinuationArena::new();
        let cont = arena.done();
        assert!(cont.is_done());
        assert_eq!(arena.len(), 1);
    }

    #[test]
    fn test_arena_sequence_empty() {
        let arena = ContinuationArena::new();
        let next = arena.done();
        let cont = arena.sequence(vec![], next.clone());
        // Empty sequence should return next directly
        assert!(Rc::ptr_eq(&cont, &next));
        assert_eq!(arena.len(), 1); // Only the done continuation
    }

    #[test]
    fn test_arena_sequence_non_empty() {
        let arena = ContinuationArena::new();
        let next = arena.done();
        let cont = arena.sequence(vec![Expr::Literal(Value::int(1))], next);
        assert!(!cont.is_done());
        assert_eq!(arena.len(), 2);
    }

    #[test]
    fn test_arena_clear() {
        let arena = ContinuationArena::new();
        arena.done();
        arena.done();
        arena.done();
        assert_eq!(arena.len(), 3);

        arena.clear();
        assert!(arena.is_empty());
    }

    #[test]
    fn test_arena_stats() {
        let arena = ContinuationArena::new();
        arena.done();
        arena.done();

        let stats = arena.stats();
        assert_eq!(stats.allocations, 2);
        assert_eq!(stats.pool_size, 2);
        assert_eq!(stats.peak_size, 2);

        arena.clear();
        let stats = arena.stats();
        assert_eq!(stats.allocations, 2); // Total doesn't reset
        assert_eq!(stats.pool_size, 0);
        assert_eq!(stats.peak_size, 2);
    }

    #[test]
    fn test_arena_with_capacity() {
        let arena = ContinuationArena::with_capacity(128);
        arena.done();
        assert_eq!(arena.len(), 1);
    }
}
