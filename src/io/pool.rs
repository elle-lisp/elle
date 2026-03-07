//! Buffer pool for async I/O backends.
//!
//! Buffers passed to io_uring must not move while the kernel holds them.
//! The pool owns Vec<u8> allocations indexed by BufferHandle. Buffers are
//! allocated on submit, returned on completion.

/// Opaque handle to a pooled buffer.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) struct BufferHandle(usize);

/// Pool of reusable byte buffers for async I/O.
///
/// Invariant: a buffer is either in `buffers[i] = Some(vec)` (allocated)
/// or its slot is in `free` (available for reuse). Never both.
pub(crate) struct BufferPool {
    buffers: Vec<Option<Vec<u8>>>,
    free: Vec<usize>,
}

impl BufferPool {
    pub(crate) fn new() -> Self {
        BufferPool {
            buffers: Vec::new(),
            free: Vec::new(),
        }
    }

    /// Allocate a buffer of `size` bytes (zeroed). Returns a handle.
    pub(crate) fn alloc(&mut self, size: usize) -> BufferHandle {
        if let Some(idx) = self.free.pop() {
            let buf = vec![0u8; size];
            self.buffers[idx] = Some(buf);
            BufferHandle(idx)
        } else {
            let idx = self.buffers.len();
            self.buffers.push(Some(vec![0u8; size]));
            BufferHandle(idx)
        }
    }

    /// Release a buffer back to the pool. Returns the buffer contents.
    ///
    /// Panics if the handle is invalid or already released.
    pub(crate) fn release(&mut self, handle: BufferHandle) -> Vec<u8> {
        let buf = self.buffers[handle.0]
            .take()
            .expect("BufferPool::release: double release or invalid handle");
        self.free.push(handle.0);
        buf
    }

    /// Get a mutable reference to the buffer contents.
    ///
    /// Panics if the handle is invalid or released.
    pub(crate) fn get_mut(&mut self, handle: BufferHandle) -> &mut Vec<u8> {
        self.buffers[handle.0]
            .as_mut()
            .expect("BufferPool::get_mut: invalid or released handle")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_returns_distinct_handles() {
        let mut pool = BufferPool::new();
        let h1 = pool.alloc(64);
        let h2 = pool.alloc(64);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_release_and_reuse() {
        let mut pool = BufferPool::new();
        let h1 = pool.alloc(64);
        pool.release(h1);
        let h2 = pool.alloc(128);
        // Reuses the same slot
        assert_eq!(h1, h2);
        // But the buffer has the new size
        assert_eq!(pool.get_mut(h2).len(), 128);
    }

    #[test]
    fn test_get_mut_returns_correct_buffer() {
        let mut pool = BufferPool::new();
        let h = pool.alloc(4);
        let buf = pool.get_mut(h);
        buf[0] = 0xAA;
        buf[1] = 0xBB;
        assert_eq!(pool.get_mut(h)[0], 0xAA);
        assert_eq!(pool.get_mut(h)[1], 0xBB);
    }

    #[test]
    fn test_alloc_zeroed() {
        let mut pool = BufferPool::new();
        let h = pool.alloc(16);
        let buf = pool.get_mut(h);
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_release_returns_contents() {
        let mut pool = BufferPool::new();
        let h = pool.alloc(4);
        pool.get_mut(h)[0] = 42;
        let returned = pool.release(h);
        assert_eq!(returned[0], 42);
    }

    #[test]
    #[should_panic(expected = "double release")]
    fn test_double_release_panics() {
        let mut pool = BufferPool::new();
        let h = pool.alloc(4);
        pool.release(h);
        pool.release(h);
    }
}
