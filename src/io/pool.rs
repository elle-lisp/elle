//! Buffer pool for async I/O backends.
//!
//! Buffers passed to io_uring must not move while the kernel holds them.
//! The pool owns `Vec<u8>` allocations indexed by BufferHandle. Buffers are
//! allocated on submit, returned on completion.

/// Opaque handle to a pooled buffer.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) struct BufferHandle(usize);

/// Pool of reusable byte buffers for async I/O.
///
/// Invariant: a buffer is either in `buffers[i] = Some(vec)` (allocated)
/// or its slot is in `free` (available for reuse). Never both.
#[allow(dead_code)]
pub(crate) struct BufferPool {
    buffers: Vec<Option<Vec<u8>>>,
    free: Vec<usize>,
}

#[allow(dead_code)]
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
mod tests;
