// audited: 2026-10-22
//! The `@string` and `@bytes` store: a byte sequence, read as a slice and edited by one splice.
//!
//! docs/impl/mutable.md

use std::cell::{Ref, RefCell};
use std::ops::Range;
use std::rc::Rc;

/// The contents of an `@string` or of `@bytes`. An `@string`'s bytes are
/// UTF-8, and the caller of a splice keeps them so.
pub struct ByteStore(Rc<RefCell<Vec<u8>>>);

/// A read of a byte store. A splice panics while one is live.
pub struct ByteRead<'a>(Ref<'a, Vec<u8>>);

impl std::ops::Deref for ByteRead<'_> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl ByteStore {
    /// A store holding `bytes`.
    pub fn new(bytes: Vec<u8>) -> Self {
        ByteStore(Rc::new(RefCell::new(bytes)))
    }

    /// Read the bytes. Panics while a splice is running.
    #[inline]
    pub fn read(&self) -> ByteRead<'_> {
        ByteRead(self.0.borrow())
    }

    /// Read the bytes, or `None` while a splice is running.
    #[inline]
    pub fn try_read(&self) -> Option<ByteRead<'_>> {
        self.0.try_borrow().ok().map(ByteRead)
    }

    /// Replace the bytes in `range` with `with`, and answer the bytes it
    /// removed. Panics when `range` is out of bounds.
    pub(in crate::value) fn splice(&self, range: Range<usize>, with: &[u8]) -> Vec<u8> {
        self.0
            .borrow_mut()
            .splice(range, with.iter().copied())
            .collect()
    }
}
