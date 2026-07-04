use super::*;
use crate::value::types::Arity;

fn test_closure() -> Rc<Closure> {
    use crate::value::ClosureTemplate;
    Rc::new(Closure {
        template: crate::value::TemplateRef::new(Rc::new(ClosureTemplate::new(
            Rc::new(vec![]),
            Arity::Exact(0),
            Rc::new(vec![]),
        ))),
        env: crate::value::region_slice::RegionSlice::empty(),
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
