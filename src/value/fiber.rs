//! Fiber types for the Elle runtime.
//!
//! A fiber is an independent execution context: it owns its operand stack,
//! call frames, and signal state. The VM dispatches into the current fiber;
//! suspended fibers are stored as heap values.

use crate::value::closure::Closure;
use crate::value::Value;
use smallvec::SmallVec;
use std::cell::RefCell;
use std::collections::HashMap;
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

/// A suspended execution point.
///
/// Captures everything needed to resume bytecode execution: the bytecode,
/// constants pool, closure environment, instruction pointer, and operand
/// stack state. Used for both signal-based suspension (`fiber/signal`) and
/// yield-based suspension (`yield` instruction).
///
/// For signal suspension, `stack` is empty (the fiber's own stack is
/// preserved). For yield suspension, `stack` captures the operand stack
/// at the point of yield.
#[derive(Debug, Clone)]
pub struct SuspendedFrame {
    /// Bytecode to resume executing
    pub bytecode: Rc<Vec<u8>>,
    /// Constants pool for this frame
    pub constants: Rc<Vec<Value>>,
    /// Closure environment
    pub env: Rc<Vec<Value>>,
    /// Instruction pointer to resume at
    pub ip: usize,
    /// Operand stack state (empty for signal suspension)
    pub stack: Vec<Value>,
}

/// Signal type bits. The first 16 are compiler-reserved.
pub type SignalBits = u32;

pub const SIG_OK: SignalBits = 0; // no bits set = normal return
pub const SIG_ERROR: SignalBits = 1 << 0; // exception / panic
pub const SIG_YIELD: SignalBits = 1 << 1; // cooperative suspension
pub const SIG_DEBUG: SignalBits = 1 << 2; // breakpoint / trace
pub const SIG_RESUME: SignalBits = 1 << 3; // fiber resumption (VM-internal)
pub const SIG_FFI: SignalBits = 1 << 4; // calls foreign code
pub const SIG_PROPAGATE: SignalBits = 1 << 5; // re-raise caught signal (VM-internal)
pub const SIG_CANCEL: SignalBits = 1 << 6; // inject error into fiber (VM-internal)

// Signal bit partitioning:
//
//   Bits 0-2:   User-facing signals (error, yield, debug)
//   Bit  3:     Resume - run a suspended fiber (VM-internal)
//   Bit  4:     FFI — calls foreign code
//   Bit  5:     Propagate — re-raise caught signal (VM-internal)
//   Bit  6:     Cancel — inject error into fiber (VM-internal)
//   Bits 7-15:  Reserved for future use
//   Bits 16-31: User-defined signal types
//
// The VM dispatch loop checks all bits. User code only sees
// bits 0-2 and 16-31. Bits 3-15 are internal.

/// Fiber status. Matches Janet's model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiberStatus {
    /// Not yet started (has closure but hasn't been resumed)
    New,
    /// Currently executing (on the VM's run stack)
    Alive,
    /// Suspended by a signal (waiting for resume)
    Suspended,
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
            FiberStatus::Suspended => "suspended",
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
    pub name: String,
    pub ip: usize,
    pub frame_base: usize,
}

/// The fiber: an independent execution context.
///
/// Holds all per-execution state that was previously on the VM struct:
/// operand stack, call frames, exception handlers.
/// The VM retains only global/shared state (globals, modules, JIT cache, FFI).
pub struct Fiber {
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
    /// Cached NaN-boxed Value for the parent fiber. Set during resume chain
    /// wiring. Avoids re-allocating a HeapObject on every `fiber/parent` call.
    pub parent_value: Option<Value>,
    /// Most recently resumed child (for stack traces and resumption routing)
    pub child: Option<FiberHandle>,
    /// Cached NaN-boxed Value for the child fiber. Set during resume chain
    /// wiring. Avoids re-allocating a HeapObject on every `fiber/child` call.
    pub child_value: Option<Value>,
    /// The closure this fiber was created from
    pub closure: Rc<Closure>,
    /// Dynamic bindings (fiber-scoped state)
    pub env: Option<HashMap<u32, Value>>,
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
}

impl Fiber {
    /// Create a new fiber from a closure with the given signal mask.
    pub fn new(closure: Rc<Closure>, mask: SignalBits) -> Self {
        Fiber {
            stack: SmallVec::new(),
            frames: Vec::new(),
            status: FiberStatus::New,
            mask,
            parent: None,
            parent_value: None,
            child: None,
            child_value: None,
            closure,
            env: None,
            signal: None,
            suspended: None,
            call_depth: 0,
            call_stack: Vec::new(),
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
    use crate::effects::Effect;
    use crate::error::LocationMap;
    use crate::value::types::Arity;

    fn test_closure() -> Rc<Closure> {
        Rc::new(Closure {
            bytecode: Rc::new(vec![]),
            arity: Arity::Exact(0),
            env: Rc::new(vec![]),
            num_locals: 0,
            num_captures: 0,
            constants: Rc::new(vec![]),
            effect: Effect::none(),
            cell_params_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            jit_code: None,
            lir_function: None,
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
        assert!(fiber.env.is_none());
        assert!(fiber.signal.is_none());
    }

    #[test]
    fn test_fiber_status_transitions() {
        let mut fiber = Fiber::new(test_closure(), 0);
        assert_eq!(fiber.status, FiberStatus::New);

        fiber.status = FiberStatus::Alive;
        assert_eq!(fiber.status, FiberStatus::Alive);

        fiber.status = FiberStatus::Suspended;
        fiber.signal = Some((SIG_YIELD, Value::int(42)));
        assert_eq!(fiber.status, FiberStatus::Suspended);
        assert_eq!(fiber.signal, Some((SIG_YIELD, Value::int(42))));

        fiber.status = FiberStatus::Dead;
        fiber.signal = Some((SIG_OK, Value::int(99)));
        assert_eq!(fiber.status, FiberStatus::Dead);
        assert_eq!(fiber.signal, Some((SIG_OK, Value::int(99))));

        // Reset and test error path
        let mut fiber2 = Fiber::new(test_closure(), 0);
        fiber2.status = FiberStatus::Error;
        fiber2.signal = Some((SIG_ERROR, Value::string("boom")));
        assert_eq!(fiber2.status, FiberStatus::Error);
    }

    #[test]
    fn test_fiber_stack_operations() {
        let mut fiber = Fiber::new(test_closure(), 0);
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
        let mut fiber = Fiber::new(closure.clone(), 0);

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
        let parent_handle = FiberHandle::new(Fiber::new(test_closure(), 0));
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
        let handle = FiberHandle::new(Fiber::new(test_closure(), 0));
        let _f1 = handle.take();
        let _f2 = handle.take(); // should panic
    }

    #[test]
    #[should_panic(expected = "slot already occupied")]
    fn test_fiber_handle_double_put_panics() {
        let handle = FiberHandle::new(Fiber::new(test_closure(), 0));
        let fiber = Fiber::new(test_closure(), 0);
        handle.put(fiber); // should panic — slot already occupied
    }

    #[test]
    fn test_signal_bits() {
        assert_eq!(SIG_OK, 0);
        assert_eq!(SIG_ERROR, 1);
        assert_eq!(SIG_YIELD, 2);
        assert_eq!(SIG_DEBUG, 4);
        assert_eq!(SIG_RESUME, 8);

        // Mask catches error and yield but not debug
        let mask = SIG_ERROR | SIG_YIELD;
        assert_ne!(mask & SIG_ERROR, 0);
        assert_ne!(mask & SIG_YIELD, 0);
        assert_eq!(mask & SIG_DEBUG, 0);
        assert_eq!(mask & SIG_RESUME, 0);

        // User-defined signals in upper 16 bits
        let user_sig: SignalBits = 1 << 16;
        assert_eq!(user_sig & mask, 0);
    }

    #[test]
    fn test_fiber_status_display() {
        assert_eq!(FiberStatus::New.as_str(), "new");
        assert_eq!(FiberStatus::Alive.as_str(), "alive");
        assert_eq!(FiberStatus::Suspended.as_str(), "suspended");
        assert_eq!(FiberStatus::Dead.as_str(), "dead");
        assert_eq!(FiberStatus::Error.as_str(), "error");
    }

    #[test]
    fn test_fiber_debug_format() {
        let fiber = Fiber::new(test_closure(), 0);
        let debug = format!("{:?}", fiber);
        assert!(debug.contains("fiber:new"));
        assert!(debug.contains("frames=0"));
        assert!(debug.contains("stack=0"));
    }

    #[test]
    fn test_fiber_zero_mask() {
        // A fiber with mask=0 propagates all signals
        let fiber = Fiber::new(test_closure(), 0);
        assert_eq!(fiber.mask & SIG_ERROR, 0);
        assert_eq!(fiber.mask & SIG_YIELD, 0);
    }

    #[test]
    fn test_fiber_full_mask() {
        // A fiber with all bits set catches everything
        let fiber = Fiber::new(test_closure(), u32::MAX);
        assert_ne!(fiber.mask & SIG_ERROR, 0);
        assert_ne!(fiber.mask & SIG_YIELD, 0);
        assert_ne!(fiber.mask & SIG_DEBUG, 0);
        assert_ne!(fiber.mask & SIG_RESUME, 0);
    }
}
