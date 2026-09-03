use super::*;

/// A fiber whose body never runs still names a code object; the instance's
/// placeholder is exactly that. The heap is leaked so the placeholder stays
/// resident for the test.
fn test_closure() -> Rc<Closure> {
    let heap = crate::value::arena::leaked_test_heap();
    noop_closure(unsafe { &mut *heap })
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

/// The compiler-reserved bit assignments the bytecode encoding depends on.
/// The set algebra over them lives in `signalbits/tests.rs`.
#[test]
fn test_signal_bit_positions() {
    assert_eq!(SIG_OK.raw(), 0);
    assert_eq!(SIG_ERROR.raw(), 1);
    assert_eq!(SIG_YIELD.raw(), 2);
    assert_eq!(SIG_DEBUG.raw(), 4);
    assert_eq!(SIG_RESUME.raw(), 8);
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
    assert!(!fiber.mask.intersects(SIG_ERROR));
    assert!(!fiber.mask.intersects(SIG_YIELD));
}

#[test]
fn test_fiber_full_mask() {
    // A fiber with all bits set catches everything
    let fiber = Fiber::new(test_closure(), SignalBits::ALL);
    assert!(fiber.mask.intersects(SIG_ERROR));
    assert!(fiber.mask.intersects(SIG_YIELD));
    assert!(fiber.mask.intersects(SIG_DEBUG));
    assert!(fiber.mask.intersects(SIG_RESUME));
}
