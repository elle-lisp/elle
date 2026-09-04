//! Where syntax nodes are allocated.
//!
//! A [`Syntax`] node is region data, so every constructor names a region.
//! [`SyntaxArena`] is that name: a `Copy` handle over a heap and one region on
//! it. It names a region; it does not own one.
//!
//! An instance runs two syntax regions — a transient working region per
//! compilation unit, and one process-root region for macro templates — and one
//! rule keeps every pointer valid: a node may point into its own arena or into
//! the template arena, and nowhere else. docs/impl/syntax.md § "Where a node
//! lives" owns the argument.

use crate::hir::region::RuntimeRegion;
use crate::value::fiberheap::FiberHeap;
use crate::value::region_slice::{RegionSlice, RegionStr};

use super::Syntax;

/// A heap and a region on it: the allocation target every syntax constructor
/// takes.
///
/// `Copy`, so it threads through the reader, the expander, and the analyzer
/// without borrowing anything. The heap is a raw pointer for the same reason
/// `VM::heap_ptr` is one: an expansion holds a `&mut VM` and allocates syntax
/// at the same time, and the two would alias if this held a borrow.
#[derive(Clone, Copy)]
pub struct SyntaxArena {
    heap: *mut FiberHeap,
    region: RuntimeRegion,
}

impl SyntaxArena {
    /// Name an existing region on `heap` as an allocation target.
    pub fn new(heap: &mut FiberHeap, region: RuntimeRegion) -> Self {
        SyntaxArena {
            heap: heap as *mut FiberHeap,
            region,
        }
    }

    /// Same, from a heap pointer the caller already holds (the VM's
    /// `heap_ptr`, or a `CompileCtx`'s).
    ///
    /// # Safety
    /// `heap` must point at a live `FiberHeap` for as long as the arena, and
    /// every node built through it, is used.
    pub unsafe fn from_raw(heap: *mut FiberHeap, region: RuntimeRegion) -> Self {
        SyntaxArena { heap, region }
    }

    /// Mint a fresh region on `heap` and name it. The caller owns the region:
    /// nothing here frees it.
    pub fn mint(heap: &mut FiberHeap) -> Self {
        let region = heap.new_runtime_region();
        SyntaxArena::new(heap, region)
    }

    /// The instance's process-lifetime syntax arena, where macro templates
    /// live.
    ///
    /// This is the heap's root region — already a process root, so teardown
    /// releases it by RC and the macro-scope reclaim protects it. A template
    /// outlives the compilation unit that defined it and every later unit that
    /// expands it, which is exactly what the root region promises.
    ///
    /// # Safety
    /// `heap` must point at a live `FiberHeap` for the arena's whole use.
    pub unsafe fn templates(heap: *mut FiberHeap) -> Self {
        let region = crate::value::arena::root_region(&mut *heap);
        SyntaxArena { heap, region }
    }

    /// The region nodes built through this arena are born in.
    pub fn region(&self) -> RuntimeRegion {
        self.region
    }

    /// The heap this arena's region lives on.
    pub fn heap_ptr(&self) -> *mut FiberHeap {
        self.heap
    }

    /// Copy `items` into the region and return the slice.
    pub(crate) fn nodes(&self, items: &[Syntax]) -> RegionSlice<Syntax> {
        if items.is_empty() {
            return RegionSlice::empty();
        }
        unsafe { (*self.heap).alloc_region_slice_in_region(items, self.region) }
    }

    /// Copy one node into the region and return a reference to it.
    pub(crate) fn node(&self, item: Syntax) -> super::SynRef {
        let slice = self.nodes(std::slice::from_ref(&item));
        // A one-element slice IS the node's home: `as_ptr` names it, and the
        // region owns the bytes.
        unsafe { super::SynRef::from_raw(slice.as_ptr()) }
    }

    /// Copy `s` into the region and return the region-resident string.
    pub(crate) fn text(&self, s: &str) -> RegionStr {
        if s.is_empty() {
            return RegionStr::empty();
        }
        let bytes = unsafe { (*self.heap).alloc_region_slice_in_region(s.as_bytes(), self.region) };
        // Valid UTF-8: the bytes are a byte-for-byte copy of a `&str`.
        unsafe { RegionStr::from_utf8_slice(bytes) }
    }

    /// Copy a scope set into the region.
    pub(crate) fn scopes(&self, scopes: &[super::ScopeId]) -> RegionSlice<super::ScopeId> {
        if scopes.is_empty() {
            return RegionSlice::empty();
        }
        unsafe { (*self.heap).alloc_region_slice_in_region(scopes, self.region) }
    }
}

impl std::fmt::Debug for SyntaxArena {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SyntaxArena(region {})", self.region)
    }
}

/// A syntax arena and the heap under it, owned together.
///
/// For callers that parse without a runtime: `elle fmt`, the pre-VM `unicode!`
/// prescan, the epoch rewriter's template parse, and unit tests. A caller that
/// has a `Runtime` uses its heap instead — one instance, one heap
/// (docs/impl/region/tls.md).
pub struct SyntaxHeap {
    heap: Box<FiberHeap>,
    region: RuntimeRegion,
}

impl SyntaxHeap {
    pub fn new() -> Self {
        let mut heap = Box::new(FiberHeap::new());
        let region = heap.new_runtime_region();
        SyntaxHeap { heap, region }
    }

    /// The arena to build nodes through. Every node it builds dies with this
    /// `SyntaxHeap`.
    pub fn arena(&mut self) -> SyntaxArena {
        SyntaxArena::new(&mut self.heap, self.region)
    }

    /// A fresh heap and an arena on it, as one pair.
    ///
    /// The heap is boxed, so its address survives the move out of this
    /// function and the arena stays valid. Keep the `SyntaxHeap` alive for as
    /// long as any node built through the arena.
    pub fn with_arena() -> (SyntaxHeap, SyntaxArena) {
        let mut home = SyntaxHeap::new();
        let arena = home.arena();
        (home, arena)
    }
}

impl Default for SyntaxHeap {
    fn default() -> Self {
        SyntaxHeap::new()
    }
}

impl Drop for SyntaxHeap {
    fn drop(&mut self) {
        self.heap.decref_region_if_present(self.region);
    }
}

thread_local! {
    /// The heap behind [`thread_arena`], one per thread.
    static THREAD_HEAP: std::cell::RefCell<SyntaxHeap> =
        std::cell::RefCell::new(SyntaxHeap::new());
}

/// An arena on a heap that lives as long as the calling thread.
///
/// Scaffolding, for tests and for one-shot tools that read some syntax, look
/// at it, and finish. It keeps a helper argument-free, and it pays for that by
/// reclaiming nothing until the thread ends. A path that runs repeatedly mints
/// its own region and frees it — see `pipeline::compile`'s
/// `with_syntax_arena`.
pub fn thread_arena() -> SyntaxArena {
    THREAD_HEAP.with(|h| h.borrow_mut().arena())
}
