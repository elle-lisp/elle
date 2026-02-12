//! Continuation pool for efficient allocation during JIT execution
//!
//! Continuations are allocated frequently during CPS execution.
//! A thread-local pool reduces allocation overhead.

use super::Continuation;
use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    /// Thread-local continuation pool for JIT execution
    static CONT_POOL: RefCell<ContinuationPool> = RefCell::new(ContinuationPool::new());
}

/// Pool of pre-allocated continuations
pub struct ContinuationPool {
    /// Pool of Done continuations (reusable)
    done_pool: Vec<Rc<Continuation>>,
    /// Statistics
    allocations: usize,
    reuses: usize,
}

impl ContinuationPool {
    /// Create a new pool
    pub fn new() -> Self {
        Self {
            done_pool: Vec::with_capacity(16),
            allocations: 0,
            reuses: 0,
        }
    }

    /// Get a Done continuation (may reuse from pool)
    pub fn get_done(&mut self) -> Rc<Continuation> {
        if let Some(cont) = self.done_pool.pop() {
            self.reuses += 1;
            cont
        } else {
            self.allocations += 1;
            Rc::new(Continuation::Done)
        }
    }

    /// Return a Done continuation to the pool
    pub fn return_done(&mut self, cont: Rc<Continuation>) {
        if cont.is_done() && self.done_pool.len() < 64 {
            self.done_pool.push(cont);
        }
    }

    /// Get allocation statistics
    pub fn stats(&self) -> (usize, usize) {
        (self.allocations, self.reuses)
    }

    /// Clear the pool
    pub fn clear(&mut self) {
        self.done_pool.clear();
    }
}

impl Default for ContinuationPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Get a Done continuation from the thread-local pool
pub fn get_done_continuation() -> Rc<Continuation> {
    CONT_POOL.with(|pool| pool.borrow_mut().get_done())
}

/// Return a continuation to the pool
pub fn return_continuation(cont: Rc<Continuation>) {
    CONT_POOL.with(|pool| pool.borrow_mut().return_done(cont));
}

/// Clear the thread-local pool
pub fn clear_pool() {
    CONT_POOL.with(|pool| pool.borrow_mut().clear());
}

/// Get pool statistics (allocations, reuses)
pub fn pool_stats() -> (usize, usize) {
    CONT_POOL.with(|pool| pool.borrow().stats())
}

/// Runtime helper: allocate a Done continuation
#[no_mangle]
pub extern "C" fn jit_alloc_done_continuation() -> i64 {
    let cont = get_done_continuation();
    Rc::into_raw(cont) as i64
}

/// Runtime helper: release a continuation
#[no_mangle]
pub extern "C" fn jit_release_continuation(cont_ptr: i64) {
    if cont_ptr != 0 {
        let cont = unsafe { Rc::from_raw(cont_ptr as *const Continuation) };
        return_continuation(cont);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_allocate() {
        let mut pool = ContinuationPool::new();
        let cont = pool.get_done();
        assert!(cont.is_done());
        assert_eq!(pool.stats(), (1, 0));
    }

    #[test]
    fn test_pool_reuse() {
        let mut pool = ContinuationPool::new();
        let cont1 = pool.get_done();
        pool.return_done(cont1);
        let _cont2 = pool.get_done();
        assert_eq!(pool.stats(), (1, 1));
    }

    #[test]
    fn test_thread_local_pool() {
        clear_pool();
        let cont = get_done_continuation();
        assert!(cont.is_done());
        return_continuation(cont);
        let (allocs, _reuses) = pool_stats();
        assert!(allocs >= 1);
    }

    #[test]
    fn test_jit_alloc_done() {
        let ptr = jit_alloc_done_continuation();
        assert_ne!(ptr, 0);
        jit_release_continuation(ptr);
    }
}
