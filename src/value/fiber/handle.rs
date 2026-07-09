//! Fiber handles: take/put ownership (`FiberHandle`) and the weak
//! parent back-pointer (`WeakFiberHandle`) that avoids Rc cycles.

use super::Fiber;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

/// A handle to a fiber that supports take/put semantics.
///
/// Wraps `Rc<RefCell<Option<Fiber>>>`. The `Option` makes "fiber is currently
/// executing on the VM" representable as `None` — no dummy fiber needed.
///
/// - `take()` extracts the fiber (sets slot to None)
/// - `put()` returns the fiber (sets slot to Some)
/// - `with()`/`with_mut()` borrow in-place for read/write
/// - `try_with()` returns None if the fiber is taken or already borrowed
#[derive(Clone)]
pub struct FiberHandle(Rc<RefCell<Option<Fiber>>>);

impl FiberHandle {
    /// Create a new handle wrapping a fiber.
    pub fn new(fiber: Fiber) -> Self {
        FiberHandle(Rc::new(RefCell::new(Some(fiber))))
    }

    /// Take the fiber out of the handle. Panics if already taken.
    pub fn take(&self) -> Fiber {
        self.0
            .borrow_mut()
            .take()
            .expect("FiberHandle::take: fiber already taken (currently executing on VM)")
    }

    /// Stable identity for this fiber (Rc pointer address).
    /// Used by the WASM backend to key per-fiber suspension frame storage.
    pub fn id(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }

    /// Put a fiber back into the handle. Panics if slot is occupied.
    pub fn put(&self, fiber: Fiber) {
        let mut slot = self.0.borrow_mut();
        assert!(
            slot.is_none(),
            "FiberHandle::put: slot already occupied (fiber not taken)"
        );
        *slot = Some(fiber);
    }

    /// Borrow the fiber immutably. Panics if taken.
    pub fn with<R>(&self, f: impl FnOnce(&Fiber) -> R) -> R {
        let borrow = self.0.borrow();
        let fiber = borrow
            .as_ref()
            .expect("FiberHandle::with: fiber is taken (currently executing on VM)");
        f(fiber)
    }

    /// Borrow the fiber mutably. Panics if taken.
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut Fiber) -> R) -> R {
        let mut borrow = self.0.borrow_mut();
        let fiber = borrow
            .as_mut()
            .expect("FiberHandle::with_mut: fiber is taken (currently executing on VM)");
        f(fiber)
    }

    /// Try to borrow the fiber immutably. Returns None if taken or already
    /// mutably borrowed (used by Debug/Display where panicking is wrong).
    pub fn try_with<R>(&self, f: impl FnOnce(&Fiber) -> R) -> Option<R> {
        let borrow = self.0.try_borrow().ok()?;
        let fiber = borrow.as_ref()?;
        Some(f(fiber))
    }

    /// Create a weak reference to this handle.
    pub fn downgrade(&self) -> WeakFiberHandle {
        WeakFiberHandle(Rc::downgrade(&self.0))
    }
}

impl std::fmt::Debug for FiberHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.try_with(|fib| fib.status.as_str().to_string()) {
            Some(status) => write!(f, "<fiber-handle:{}>", status),
            None => write!(f, "<fiber-handle:taken>"),
        }
    }
}

/// A weak reference to a FiberHandle, used for parent back-pointers
/// to avoid Rc cycles.
#[derive(Clone)]
pub struct WeakFiberHandle(Weak<RefCell<Option<Fiber>>>);

impl WeakFiberHandle {
    /// Attempt to upgrade to a strong FiberHandle. Returns None if the
    /// fiber has been dropped.
    pub fn upgrade(&self) -> Option<FiberHandle> {
        self.0.upgrade().map(FiberHandle)
    }
}

impl std::fmt::Debug for WeakFiberHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<weak-fiber-handle>")
    }
}
