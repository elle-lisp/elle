//! Fiber types for the Elle runtime.
//!
//! A fiber is an independent execution context: it owns its operand stack,
//! call frames, and signal state. The VM dispatches into the current fiber;
//! suspended fibers are stored as heap values.

use crate::error::LocationMap;
use crate::value::closure::Closure;
use crate::value::Value;
use smallvec::SmallVec;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

// ---------------------------------------------------------------------------
// FiberHandle / WeakFiberHandle
// ---------------------------------------------------------------------------

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

/// A suspended bytecode execution point.
///
/// Captures everything needed to resume bytecode execution: the bytecode,
/// constants pool, closure environment, instruction pointer, and operand
/// stack state. Used for both signal-based suspension (`fiber/signal`) and
/// yield-based suspension (`yield` instruction).
///
/// `stack` always captures the full operand stack at the moment of suspension.
/// For yield suspension, `ip` points past the `Yield` instruction and the
/// resume value needs to be pushed as the result of the `(yield ...)` expression.
/// For instruction-pause suspension (fuel, signal), `ip` points at the paused
/// instruction and the stack is already complete — no extra value is pushed.
/// The `push_resume_value` field encodes which case applies.
#[derive(Debug, Clone)]
pub struct BytecodeFrame {
    /// Bytecode to resume executing
    pub bytecode: Rc<Vec<u8>>,
    /// Constants pool for this frame
    pub constants: Rc<Vec<Value>>,
    /// Closure environment
    pub env: Rc<Vec<Value>>,
    /// Instruction pointer to resume at
    pub ip: usize,
    /// Operand stack state at suspension
    pub stack: Vec<Value>,
    /// Location map for mapping bytecode offsets to source locations
    pub location_map: Rc<LocationMap>,
    /// Whether to push `current_value` onto the stack before resuming.
    ///
    /// `true` for yield frames and caller frames: the resume value is the
    /// "return value" of the suspended operation (the yield expression result,
    /// or the return value of a call).  `false` for fuel-pause and
    /// signal-pause frames: the instruction at `ip` re-executes from scratch
    /// with the stack exactly as saved — no extra value is injected.
    pub push_resume_value: bool,
}

/// A suspended execution step — either a bytecode frame or a sub-fiber resume.
///
/// The `suspended` Vec on a `Fiber` contains a chain of these, replayed
/// innermost-first by `resume_suspended`.
///
/// - `Bytecode`: resume bytecode execution at a saved instruction pointer.
/// - `FiberResume`: resume a suspended sub-fiber (e.g. a `defer`/`protect`
///   body fiber) with the value flowing through the chain.  This is used when
///   a sub-fiber's I/O signal propagates through its parent: the parent saves
///   a `FiberResume` frame so that on re-entry the I/O result is delivered to
///   the sub-fiber first, and the sub-fiber's final return value then flows
///   into the next frame in the chain (typically the outer `BytecodeFrame`
///   that continues the `defer`/`protect` expansion after `fiber/resume`).
#[derive(Debug, Clone)]
pub enum SuspendedFrame {
    /// Resume bytecode execution from a saved point.
    Bytecode(BytecodeFrame),
    /// Resume a suspended sub-fiber with the incoming value, then continue
    /// to the next frame in the chain with the sub-fiber's return value.
    FiberResume {
        /// Handle to the suspended sub-fiber.
        handle: FiberHandle,
        /// The cached `Value` wrapping `handle` (for child-chain wiring).
        fiber_value: Value,
    },
}

/// Signal type bits. The first 16 are compiler-reserved.
///
/// Newtype over `u64` providing named methods and bitwise operator impls.
///
/// The inner representation is an implementation detail. All code outside
/// this impl block should use the provided methods instead of accessing
/// the raw field.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SignalBits(u64);

impl SignalBits {
    // -- Constructors --------------------------------------------------------

    /// Wrap a raw bitmask.
    pub const fn new(bits: u64) -> Self {
        SignalBits(bits)
    }

    /// The empty set (no signals).
    pub const EMPTY: SignalBits = SignalBits(0);

    /// The full set (all bits set).
    pub const ALL: SignalBits = SignalBits(!0);

    /// A single-bit mask for bit position `pos`.
    pub const fn from_bit(pos: u32) -> Self {
        SignalBits(1u64 << pos)
    }

    /// Construct from an i64 (e.g. from an Elle integer value).
    pub const fn from_i64(v: i64) -> Self {
        SignalBits(v as u64)
    }

    // -- Predicates ----------------------------------------------------------

    /// True when no bits are set (normal return / no signals).
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Alias for `is_empty` — reads better in dispatch contexts.
    pub const fn is_ok(self) -> bool {
        self.0 == 0
    }

    /// True when `self` and `other` share at least one bit.
    pub const fn intersects(self, other: SignalBits) -> bool {
        self.0 & other.0 != 0
    }

    /// Alias for `intersects` — existing API, kept for compatibility.
    pub const fn contains(self, other: SignalBits) -> bool {
        self.0 & other.0 != 0
    }

    /// True when `self` has bit at position `pos` set.
    pub const fn has_bit(self, pos: u32) -> bool {
        self.0 & (1 << pos) != 0
    }

    /// True iff this mask handles `other` for signal routing purposes.
    ///
    /// Uses overlap (any shared bit) for semantic bits, but requires full
    /// containment of infrastructure bits (specifically SIG_IO). This
    /// ensures that a coroutine with mask SIG_YIELD does not accidentally
    /// swallow SIG_YIELD|SIG_IO signals that must reach the scheduler,
    /// while still allowing user-defined compound signals (e.g. |:log :audit|)
    /// to be caught by a partial mask (e.g. |:log|).
    pub fn covers(self, other: SignalBits) -> bool {
        use crate::signals::SIG_IO;
        other.is_ok()
            || (self.intersects(other) && (!other.contains(SIG_IO) || self.contains(SIG_IO)))
    }

    // -- Combining -----------------------------------------------------------

    /// Bitwise OR (const-compatible union).
    pub const fn union(self, other: SignalBits) -> Self {
        SignalBits(self.0 | other.0)
    }

    /// Bitwise AND (const-compatible intersection).
    pub const fn intersection(self, other: SignalBits) -> Self {
        SignalBits(self.0 & other.0)
    }

    /// Bits in `self` that are NOT in `other` (const-compatible set difference).
    pub const fn subtract(self, other: SignalBits) -> Self {
        SignalBits(self.0 & !other.0)
    }

    /// Bitwise complement.
    pub const fn complement(self) -> Self {
        SignalBits(!self.0)
    }

    // -- Conversion / inspection ---------------------------------------------

    /// Position of the lowest set bit (for single-bit values).
    pub const fn trailing_zeros(self) -> u32 {
        self.0.trailing_zeros()
    }

    /// Raw bits as `u64`. Prefer named methods; use this only for
    /// serialization, FFI, or bytecode encoding.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl std::ops::BitOr for SignalBits {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        SignalBits(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for SignalBits {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        SignalBits(self.0 & rhs.0)
    }
}

impl std::ops::BitOrAssign for SignalBits {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAndAssign for SignalBits {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl std::ops::Not for SignalBits {
    type Output = Self;
    fn not(self) -> Self {
        SignalBits(!self.0)
    }
}

impl std::fmt::Debug for SignalBits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SignalBits(0x{:x})", self.0)
    }
}

impl std::fmt::Display for SignalBits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{:x}", self.0)
    }
}

impl From<u64> for SignalBits {
    fn from(v: u64) -> Self {
        SignalBits::new(v)
    }
}

impl From<u32> for SignalBits {
    fn from(v: u32) -> Self {
        SignalBits::new(v as u64)
    }
}

impl From<SignalBits> for u64 {
    fn from(v: SignalBits) -> u64 {
        v.raw()
    }
}

// Signal constants are canonically defined in `crate::signals` (the semantic
// owner). Re-exported here so existing `use crate::value::fiber::SIG_*`
// imports continue to work.
pub use crate::signals::{
    SIG_ABORT, SIG_DEBUG, SIG_ERROR, SIG_EXEC, SIG_FFI, SIG_FUEL, SIG_HALT, SIG_IO, SIG_OK,
    SIG_PROPAGATE, SIG_QUERY, SIG_RESUME, SIG_SWITCH, SIG_TERMINAL, SIG_WAIT, SIG_YIELD,
};

/// Fiber lifecycle status. Diverges from Janet: caught SIG_ERROR leaves
/// fiber Suspended (resumable), not Error. See vm/fiber.rs for details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiberStatus {
    /// Not yet started (has closure but hasn't been resumed)
    New,
    /// Currently executing (on the VM's run stack)
    Alive,
    /// Paused by a signal (waiting for resume)
    Paused,
    /// Completed normally (returned a value)
    Dead,
    /// Terminated by an unhandled error signal
    Error,
}

impl FiberStatus {
    /// Human-readable name for display formatting.
    pub fn as_str(self) -> &'static str {
        match self {
            FiberStatus::New => "new",
            FiberStatus::Alive => "alive",
            FiberStatus::Paused => "paused",
            FiberStatus::Dead => "dead",
            FiberStatus::Error => "error",
        }
    }
}

/// A single call frame within a fiber (for execution dispatch).
#[derive(Debug, Clone)]
pub struct Frame {
    /// The closure being executed
    pub closure: Rc<Closure>,
    /// Instruction pointer (byte offset into bytecode)
    pub ip: usize,
    /// Base index in the fiber's operand stack for this frame's temporaries
    pub base: usize,
}

/// Call frame for stack traces (name + ip + frame_base).
/// Separate from Frame because stack traces need human-readable names,
/// while execution dispatch needs closure references.
#[derive(Debug, Clone)]
pub struct CallFrame {
    pub name: Rc<str>,
    pub ip: usize,
    pub frame_base: usize,
    pub location_map: Rc<crate::error::LocationMap>,
}

/// The fiber: an independent execution context.
///
/// Holds all per-execution state that was previously on the VM struct:
/// operand stack, call frames, exception handlers.
/// The VM retains only shared state (modules, JIT cache, FFI, docs).
pub struct Fiber {
    /// Per-fiber heap for arena-style allocation. Boxed for pointer stability:
    /// the thread-local stores `*mut FiberHeap`, which must survive moves of
    /// the Fiber struct (e.g., during `std::mem::swap` in fiber transitions).
    ///
    /// **Child fibers**: this is the active allocator for the fiber's lifetime.
    /// Installed as the current thread-local heap on resume, uninstalled on
    /// suspend/death.
    ///
    /// **Root fiber**: this field is structurally present for uniformity but is
    /// never installed as the active allocator. The root fiber allocates through
    /// the `ROOT_HEAP` thread-local in `src/value/fiberheap/routing.rs`, which
    /// is a separately leaked `Box<FiberHeap>` that lives for the thread's
    /// lifetime. The root `Fiber` struct's `heap` field is constructed in
    /// `Fiber::new()` but immediately superseded by `install_root_heap()` in
    /// `VM::new()`.
    ///
    /// `Option<Box<FiberHeap>>` was considered and rejected: child fibers access
    /// `self.heap` on every allocation-related path, and wrapping in `Option`
    /// would require `.as_ref().unwrap()` or `.as_deref_mut().unwrap()` at every
    /// such call site with no benefit (child fibers always have a heap).
    pub heap: Box<crate::value::fiberheap::FiberHeap>,
    /// Operand stack (temporaries). SmallVec avoids heap allocation for
    /// fibers with fewer than 256 stack entries.
    pub stack: SmallVec<[Value; 256]>,
    /// Call frame stack (for fiber execution — closure + ip + base)
    pub frames: Vec<Frame>,
    /// Current status
    pub status: FiberStatus,
    /// Signal mask: which of this fiber's signals are caught by its parent.
    /// Set at creation time by the parent. Immutable after creation.
    pub mask: SignalBits,
    /// Parent fiber (weak to avoid Rc cycles)
    pub parent: Option<WeakFiberHandle>,
    /// Cached Value for the parent fiber. Set during resume chain
    /// wiring. Avoids re-allocating a HeapObject on every `fiber/parent` call.
    pub parent_value: Option<Value>,
    /// Most recently resumed child (for stack traces and resumption routing)
    pub child: Option<FiberHandle>,
    /// Cached Value for the child fiber. Set during resume chain
    /// wiring. Avoids re-allocating a HeapObject on every `fiber/child` call.
    pub child_value: Option<Value>,
    /// The closure this fiber was created from
    pub closure: Rc<Closure>,
    /// Parameter binding frames. Each `parameterize` pushes a frame;
    /// exiting pops it. Lookup walks frames from top to bottom.
    pub param_frames: Vec<Vec<(u32, Value)>>,
    /// Signal value from this fiber. Canonical location for both
    /// signal payloads and normal return values.
    /// - On signal: (bits, payload) before suspending
    /// - On normal return: (SIG_OK, return_value) before completing
    pub signal: Option<(SignalBits, Value)>,
    /// Suspended execution frames. Set when the fiber suspends; consumed
    /// when it resumes.
    ///
    /// - Signal suspension (`fiber/signal`): single frame, empty stack
    /// - Yield suspension (`yield`): chain of frames from yielder to
    ///   coroutine boundary, each with its operand stack captured
    ///
    /// On resume, frames are replayed from innermost (index 0) to
    /// outermost (last index).
    pub suspended: Option<Vec<SuspendedFrame>>,

    // --- Execution state migrated from VM ---
    /// Call depth counter (for stack overflow detection)
    pub call_depth: usize,
    /// Call stack for stack traces (name + ip + frame_base)
    pub call_stack: Vec<CallFrame>,
    /// Instruction budget. `None` = unlimited (default). `Some(n)` = `n` units
    /// remaining. Decremented at backward jumps and call instructions. When it
    /// reaches zero the VM emits `SIG_FUEL`, pausing the fiber. Refuel via
    /// `fiber/set-fuel` then call `fiber/resume` to continue.
    pub fuel: Option<u32>,
    /// Withheld capabilities. Bits set here prevent the fiber from silently
    /// performing the corresponding operations. When a primitive's signal bits
    /// overlap with `withheld & CAP_MASK`, the primitive is blocked and a
    /// denial signal is emitted instead. Default: empty (full access).
    /// Transitive: `child.withheld = parent.withheld | deny_bits`.
    pub withheld: SignalBits,
}

impl Fiber {
    /// Create a new fiber from a closure with the given signal mask.
    pub fn new(closure: Rc<Closure>, mask: SignalBits) -> Self {
        Fiber {
            heap: Box::new(crate::value::fiberheap::FiberHeap::new()),
            stack: SmallVec::new(),
            frames: Vec::new(),
            status: FiberStatus::New,
            mask,
            parent: None,
            parent_value: None,
            child: None,
            child_value: None,
            closure,
            param_frames: Vec::new(),
            signal: None,
            suspended: None,
            call_depth: 0,
            call_stack: Vec::new(),
            fuel: None,
            withheld: SignalBits::EMPTY,
        }
    }
}

impl std::fmt::Debug for Fiber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<fiber:{} frames={} stack={}>",
            self.status.as_str(),
            self.frames.len(),
            self.stack.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::LocationMap;
    use crate::signals::Signal;
    use crate::value::types::Arity;
    use std::collections::HashMap;

    fn test_closure() -> Rc<Closure> {
        use crate::value::ClosureTemplate;
        Rc::new(Closure {
            template: Rc::new(ClosureTemplate {
                bytecode: Rc::new(vec![]),
                arity: Arity::Exact(0),
                num_locals: 0,
                num_captures: 0,
                num_params: 0,
                constants: Rc::new(vec![]),
                signal: Signal::silent(),
                capture_params_mask: 0,
                capture_locals_mask: 0,

                symbol_names: Rc::new(HashMap::new()),
                location_map: Rc::new(LocationMap::new()),
                rotation_safe: false,
                lir_function: None,
                doc: None,
                syntax: None,
                vararg_kind: crate::hir::VarargKind::List,
                name: None,
                result_is_immediate: false,
                has_outward_heap_set: false,
                wasm_func_idx: None,
                spirv: std::cell::OnceCell::new(),
            }),
            env: crate::value::inline_slice::InlineSlice::empty(),
            squelch_mask: SignalBits::EMPTY,
        })
    }

    #[test]
    fn test_fiber_new() {
        let fiber = Fiber::new(test_closure(), SIG_ERROR | SIG_YIELD);
        assert_eq!(fiber.status, FiberStatus::New);
        assert_eq!(fiber.mask, SIG_ERROR | SIG_YIELD);
        assert!(fiber.stack.is_empty());
        assert!(fiber.frames.is_empty());
        assert!(fiber.parent.is_none());
        assert!(fiber.child.is_none());
        assert!(fiber.param_frames.is_empty());
        assert!(fiber.signal.is_none());
    }

    #[test]
    fn test_fiber_status_transitions() {
        let mut fiber = Fiber::new(test_closure(), SIG_OK);
        assert_eq!(fiber.status, FiberStatus::New);

        fiber.status = FiberStatus::Alive;
        assert_eq!(fiber.status, FiberStatus::Alive);

        fiber.status = FiberStatus::Paused;
        fiber.signal = Some((SIG_YIELD, Value::int(42)));
        assert_eq!(fiber.status, FiberStatus::Paused);
        assert_eq!(fiber.signal, Some((SIG_YIELD, Value::int(42))));

        fiber.status = FiberStatus::Dead;
        fiber.signal = Some((SIG_OK, Value::int(99)));
        assert_eq!(fiber.status, FiberStatus::Dead);
        assert_eq!(fiber.signal, Some((SIG_OK, Value::int(99))));

        // Reset and test error path
        let mut fiber2 = Fiber::new(test_closure(), SIG_OK);
        fiber2.status = FiberStatus::Error;
        fiber2.signal = Some((SIG_ERROR, Value::string("boom")));
        assert_eq!(fiber2.status, FiberStatus::Error);
    }

    #[test]
    fn test_fiber_stack_operations() {
        let mut fiber = Fiber::new(test_closure(), SIG_OK);
        fiber.stack.push(Value::int(1));
        fiber.stack.push(Value::int(2));
        fiber.stack.push(Value::int(3));
        assert_eq!(fiber.stack.len(), 3);
        assert_eq!(fiber.stack.pop(), Some(Value::int(3)));
        assert_eq!(fiber.stack.len(), 2);
    }

    #[test]
    fn test_fiber_frame_operations() {
        let closure = test_closure();
        let mut fiber = Fiber::new(closure.clone(), SIG_OK);

        let frame = Frame {
            closure: closure.clone(),
            ip: 0,
            base: 0,
        };
        fiber.frames.push(frame);
        assert_eq!(fiber.frames.len(), 1);
        assert_eq!(fiber.frames[0].ip, 0);
        assert_eq!(fiber.frames[0].base, 0);

        let frame2 = Frame {
            closure,
            ip: 10,
            base: 3,
        };
        fiber.frames.push(frame2);
        assert_eq!(fiber.frames.len(), 2);
        assert_eq!(fiber.frames[1].ip, 10);
        assert_eq!(fiber.frames[1].base, 3);
    }

    #[test]
    fn test_fiber_parent_child() {
        let parent_handle = FiberHandle::new(Fiber::new(test_closure(), SIG_OK));
        let child_handle = FiberHandle::new(Fiber::new(test_closure(), SIG_ERROR));

        // Wire up parent/child
        child_handle.with_mut(|child| {
            child.parent = Some(parent_handle.downgrade());
        });
        parent_handle.with_mut(|parent| {
            parent.child = Some(child_handle.clone());
        });

        // Parent can reach child
        parent_handle.with(|parent| {
            assert!(parent.child.is_some());
        });

        // Child can reach parent (via upgrade)
        child_handle.with(|child| {
            let parent_ref = child.parent.as_ref().unwrap().upgrade();
            assert!(parent_ref.is_some());
        });

        // Drop parent — child's weak ref becomes invalid
        drop(parent_handle);
        child_handle.with(|child| {
            let parent_ref = child.parent.as_ref().unwrap().upgrade();
            assert!(parent_ref.is_none());
        });
    }

    #[test]
    fn test_fiber_handle_take_put() {
        let handle = FiberHandle::new(Fiber::new(test_closure(), SIG_ERROR));

        // Can read via with()
        handle.with(|f| assert_eq!(f.status, FiberStatus::New));

        // Take the fiber out
        let mut fiber = handle.take();
        assert_eq!(fiber.status, FiberStatus::New);

        // try_with returns None when taken
        assert!(handle.try_with(|f| f.status).is_none());

        // Modify and put back
        fiber.status = FiberStatus::Alive;
        handle.put(fiber);

        // Can read again
        handle.with(|f| assert_eq!(f.status, FiberStatus::Alive));
    }

    #[test]
    #[should_panic(expected = "fiber already taken")]
    fn test_fiber_handle_double_take_panics() {
        let handle = FiberHandle::new(Fiber::new(test_closure(), SIG_OK));
        let _f1 = handle.take();
        let _f2 = handle.take(); // should panic
    }

    #[test]
    #[should_panic(expected = "slot already occupied")]
    fn test_fiber_handle_double_put_panics() {
        let handle = FiberHandle::new(Fiber::new(test_closure(), SIG_OK));
        let fiber = Fiber::new(test_closure(), SIG_OK);
        handle.put(fiber); // should panic — slot already occupied
    }

    #[test]
    fn test_signal_bits() {
        assert_eq!(SIG_OK.raw(), 0);
        assert_eq!(SIG_ERROR.raw(), 1);
        assert_eq!(SIG_YIELD.raw(), 2);
        assert_eq!(SIG_DEBUG.raw(), 4);
        assert_eq!(SIG_RESUME.raw(), 8);

        // Mask catches error and yield but not debug
        let mask = SIG_ERROR | SIG_YIELD;
        assert!(mask.contains(SIG_ERROR));
        assert!(mask.contains(SIG_YIELD));
        assert!(!mask.contains(SIG_DEBUG));
        assert!(!mask.contains(SIG_RESUME));

        // User-defined signals in bits 32-63
        let user_sig = SignalBits::new(1 << 32);
        assert!(!user_sig.contains(mask));
    }

    #[test]
    fn test_signal_bits_covers() {
        // covers: exact match — mask handles exact signal
        assert!(SIG_YIELD.covers(SIG_YIELD));
        // covers: SIG_YIELD mask does NOT handle SIG_YIELD|SIG_IO (missing SIG_IO infrastructure bit)
        assert!(!SIG_YIELD.covers(SIG_YIELD | SIG_IO));
        // covers: mask with SIG_IO handles SIG_YIELD|SIG_IO (IO bit present, overlap on YIELD)
        assert!((SIG_ERROR | SIG_IO).covers(SIG_YIELD | SIG_IO));
        // covers: all-bits mask handles any compound signal
        assert!(SignalBits::new(!0).covers(SIG_YIELD | SIG_IO));
        // covers: SIG_OK (zero) is always handled by any mask
        assert!(SIG_YIELD.covers(SIG_OK));
        assert!(SIG_OK.covers(SIG_OK));
        // covers: user-defined signals use overlap semantics (no SIG_IO involved)
        // mask |:log| catches |:log :audit| because :log overlaps
        let log_bit = SignalBits::new(1 << 16);
        let audit_bit = SignalBits::new(1 << 17);
        assert!(log_bit.covers(log_bit | audit_bit));
        assert!(audit_bit.covers(log_bit | audit_bit));
        // covers: mask does not catch a completely disjoint signal
        assert!(!SIG_YIELD.covers(SIG_ERROR));
    }

    #[test]
    fn test_fiber_status_display() {
        assert_eq!(FiberStatus::New.as_str(), "new");
        assert_eq!(FiberStatus::Alive.as_str(), "alive");
        assert_eq!(FiberStatus::Paused.as_str(), "paused");
        assert_eq!(FiberStatus::Dead.as_str(), "dead");
        assert_eq!(FiberStatus::Error.as_str(), "error");
    }

    #[test]
    fn test_fiber_debug_format() {
        let fiber = Fiber::new(test_closure(), SIG_OK);
        let debug = format!("{:?}", fiber);
        assert!(debug.contains("fiber:new"));
        assert!(debug.contains("frames=0"));
        assert!(debug.contains("stack=0"));
    }

    #[test]
    fn test_fiber_zero_mask() {
        // A fiber with mask=0 propagates all signals
        let fiber = Fiber::new(test_closure(), SIG_OK);
        assert!(!fiber.mask.contains(SIG_ERROR));
        assert!(!fiber.mask.contains(SIG_YIELD));
    }

    #[test]
    fn test_fiber_full_mask() {
        // A fiber with all bits set catches everything
        let fiber = Fiber::new(test_closure(), SignalBits::new(u64::MAX));
        assert!(fiber.mask.contains(SIG_ERROR));
        assert!(fiber.mask.contains(SIG_YIELD));
        assert!(fiber.mask.contains(SIG_DEBUG));
        assert!(fiber.mask.contains(SIG_RESUME));
    }

    /// Regression: two Fiber values that wrap the *same* `FiberHandle` but
    /// are stored in distinct arena slots must compare equal and hash
    /// identically.
    ///
    /// The motivating scenario is `deep_copy_to_outbox` on fiber yield: a
    /// signal value containing a fiber gets re-allocated in the outbox,
    /// producing a new `HeapObject::Fiber` slot whose `handle` is a clone
    /// of the original. Both values represent the same fiber — the Elle
    /// scheduler stores fibers as keys in `waiters`/`completed` maps, and
    /// those lookups must hit regardless of which slot the caller holds.
    ///
    /// Historically `Value::eq`/`Value::hash` used the slot pointer, so
    /// the copied fiber became a *different* map key. That desync caused
    /// `ev/join` on a recently-spawned fiber to park forever.
    #[test]
    fn test_fiber_values_sharing_a_handle_are_identity_equal() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let handle = FiberHandle::new(Fiber::new(test_closure(), SIG_OK));

        // Two independent arena allocations of the *same* handle — this
        // is exactly what deep_copy_to_outbox produces.
        let v1 = Value::fiber_from_handle(handle.clone());
        let v2 = Value::fiber_from_handle(handle.clone());

        // Precondition: the two values really are at distinct slots.
        // (If they ever coalesce, the test becomes trivially green and
        // no longer exercises the bug.)
        assert_ne!(
            v1.payload, v2.payload,
            "precondition: two separate allocations should have distinct slot addresses"
        );

        // Same fiber => equal.
        assert_eq!(v1, v2, "fibers sharing a handle must compare equal");

        // Same fiber => same hash (Hash/Eq contract).
        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();
        v1.hash(&mut h1);
        v2.hash(&mut h2);
        assert_eq!(
            h1.finish(),
            h2.finish(),
            "fibers sharing a handle must hash identically"
        );

        // Unrelated fibers still compare not-equal.
        let other_handle = FiberHandle::new(Fiber::new(test_closure(), SIG_OK));
        let v3 = Value::fiber_from_handle(other_handle);
        assert_ne!(v1, v3, "distinct fibers must not compare equal");
    }
}
