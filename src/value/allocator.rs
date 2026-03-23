//! Custom allocator trait for `with-allocator`.
//!
//! Plugins implement `ElleAllocator` to provide custom memory backing.
//! The runtime wraps the trait object in `AllocatorBox`, which is stored
//! as an `ExternalObject` in an Elle `Value`.
//!
//! See `issue-with-allocator/design.md` for the full design.

/// Trait for custom allocators provided via plugin or native code.
///
/// Implementations must handle interior mutability themselves (`&self`
/// methods). This matches the allocation path: `FiberHeap::alloc()` takes
/// `&mut self`, but the custom allocator is behind an `Rc` that may be
/// aliased (the Elle `Value` wrapping it is `Copy`).
///
/// # Safety contract
///
/// - `alloc` must return a pointer aligned to `align` with at least
///   `size` usable bytes, or null on failure.
/// - `dealloc` receives the exact `(ptr, size, align)` triple from a
///   prior `alloc` call. The implementation must tolerate being called
///   in any order (not necessarily LIFO).
/// - Both methods may be called from any point in the Elle runtime.
///   They must NOT call back into Elle (no Value allocation, no
///   closure calls, no fiber operations).
/// - Implementations must be re-entrant with respect to themselves:
///   `dealloc` may be called while another `alloc` on the same
///   allocator is logically in progress (e.g., during destructor
///   cleanup that frees a nested custom-allocated value).
pub trait ElleAllocator: 'static {
    /// Allocate `size` bytes with alignment `align`.
    ///
    /// Returns a pointer to the allocated memory, or null on failure.
    /// On null, the runtime falls back to the root slab.
    fn alloc(&self, size: usize, align: usize) -> *mut u8;

    /// Deallocate memory previously returned by `alloc`.
    ///
    /// Called per-object on scope exit and form exit, after the object's
    /// destructor (Drop) has run.
    fn dealloc(&self, ptr: *mut u8, size: usize, align: usize);
}

/// Wrapper stored in `ExternalObject.data` that bridges `Any` to
/// `ElleAllocator`.
///
/// The plugin creates `Value::external("allocator", AllocatorBox::new(my_alloc))`.
/// The `%install-allocator` primitive does `value.as_external::<AllocatorBox>()`
/// and accesses `.inner`. Two levels of indirection (`Rc<dyn Any>` →
/// `AllocatorBox` → `Box<dyn ElleAllocator>`), but only at install time —
/// not on every allocation. At install, we clone the `Rc<AllocatorBox>`
/// into `CustomAllocState`.
pub struct AllocatorBox {
    pub(crate) inner: Box<dyn ElleAllocator>,
}

impl AllocatorBox {
    pub fn new<A: ElleAllocator>(alloc: A) -> Self {
        AllocatorBox {
            inner: Box::new(alloc),
        }
    }
}

// AllocatorBox is automatically Any since ElleAllocator: 'static
// and Box<dyn ElleAllocator>: 'static.
