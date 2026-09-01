use super::*;

#[test]
fn test_async_backend_new() {
    let backend = AsyncBackend::new();
    assert!(backend.is_ok());
}

/// The keepalive a program names reaches the crew that reads it.
///
/// `(io/backend :async k)` is the whole path from `*io-keepalive*` to the
/// worker that parks: the primitive turns the seconds into a `Duration`, the
/// backend hands it to the hub, and the hub's pool is what waits. Nothing a
/// completion carries reports the wait, so the plumbing is pinned here and the
/// behavior it buys is pinned in `src/io/threadpool/tests/pool.rs`.
///
/// The counter-factual: a backend that dropped the argument and built its hub
/// with `CompletionHub::new()` passes every other test in this file.
#[test]
fn a_backend_takes_the_keepalive_it_was_given() {
    let named = AsyncBackend::new_thread_pool_with_keepalive(Some(Duration::from_millis(250)))
        .expect("a pool backend");
    assert_eq!(named.keepalive(), Duration::from_millis(250));

    let off =
        AsyncBackend::new_thread_pool_with_keepalive(Some(Duration::ZERO)).expect("a pool backend");
    assert!(off.keepalive().is_zero(), "zero turns worker reuse off");

    let unnamed = AsyncBackend::new_thread_pool().expect("a pool backend");
    assert_eq!(
        unnamed.keepalive(),
        crate::io::threadpool::DEFAULT_KEEPALIVE,
        "a backend given no keepalive takes the default"
    );
}

#[test]
fn test_submit_returns_monotonic_ids() {
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();
        let path = write_temp_file("hello");
        let port = open_read_port(&path);

        let req1 = IoRequest {
            op: PortOp::ReadAll.into(),
            port,
            timeout: None,
        };
        let req2 = IoRequest {
            op: PortOp::ReadAll.into(),
            port,
            timeout: None,
        };

        let id1 = backend
            .submit(&req1, crate::io::pending::Submitter::for_test())
            .unwrap();
        let id2 = backend
            .submit(&req2, crate::io::pending::Submitter::for_test())
            .unwrap();
        assert!(id2 > id1, "IDs must be monotonically increasing");

        std::fs::remove_file(&path).ok();
    });
}

#[test]
fn test_submit_closed_port_errors() {
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();
        let path = write_temp_file("test");
        let port_val = open_read_port(&path);
        let port = port_val.as_external::<Port>().unwrap();
        port.close();

        let req = IoRequest {
            op: PortOp::ReadAll.into(),
            port: port_val,
            timeout: None,
        };
        let result = backend.submit(&req, crate::io::pending::Submitter::for_test());
        assert!(result.is_err());

        std::fs::remove_file(&path).ok();
    });
}

#[test]
fn test_poll_empty_when_no_completions() {
    let backend = AsyncBackend::new().unwrap();
    let completions = backend.poll();
    assert!(completions.is_empty());
}

#[test]
fn test_submit_and_wait_read() {
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();
        let path = write_temp_file("async read test");
        let port = open_read_port(&path);

        let req = IoRequest {
            op: PortOp::ReadAll.into(),
            port,
            timeout: None,
        };
        let id = backend
            .submit(&req, crate::io::pending::Submitter::for_test())
            .unwrap();

        let completions = backend.wait(-1).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        assert!(completions[0].result.is_ok());

        std::fs::remove_file(&path).ok();
    });
}

#[test]
fn test_submit_and_wait_write() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let backend = AsyncBackend::new().unwrap();
        let path = temp_path("async-write");
        let port = open_write_port(&path);

        let req = IoRequest {
            op: PortOp::Write {
                data: h.ctx().string("async write"),
            }
            .into(),
            port,
            timeout: None,
        };
        let id = backend
            .submit(&req, crate::io::pending::Submitter::for_test())
            .unwrap();

        let completions = backend.wait(-1).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        assert!(completions[0].result.is_ok());

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "async write");

        std::fs::remove_file(&path).ok();
    });
}

#[test]
fn test_completion_to_value_success() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let c = Completion::ok(SubmissionId::from_raw(42), h.ctx().string("hello"));
        let ctx = h.ctx();
        let v = c.to_value(&ctx);
        // The struct is born in the REAPING call's own region, so the array
        // `io/wait` collects it into and the struct share one region and one
        // release (docs/impl/region/ctx.md § "A helper reached from inside a
        // call allocates through THAT call's ctx").
        assert_eq!(
            crate::value::arena::region_of(h.heap(), v),
            Some(ctx.test_region()),
            "a completion struct is born in the reaping call's region"
        );
        let fields = v.as_struct().unwrap();
        assert_eq!(
            sorted_struct_get(fields, &TableKey::keyword("id"))
                .unwrap()
                .as_int(),
            Some(42)
        );
        assert!(sorted_struct_get(fields, &TableKey::keyword("error"))
            .unwrap()
            .is_nil());
    });
}

#[test]
fn test_completion_to_value_error() {
    crate::value::arena::with_test_region(|| {
        let heap_ptr = crate::value::arena::leaked_test_heap();
        let region = unsafe { (*heap_ptr).new_runtime_region() };
        let c = Completion::err(
            SubmissionId::from_raw(7),
            error_val_in(unsafe { &mut *heap_ptr }, "io-error", "test error", region),
        );
        let h = crate::primitives::ctx::TestHeap::new();
        let ctx = h.ctx();
        let v = c.to_value(&ctx);
        let fields = v.as_struct().unwrap();
        assert_eq!(
            sorted_struct_get(fields, &TableKey::keyword("id"))
                .unwrap()
                .as_int(),
            Some(7)
        );
        assert!(sorted_struct_get(fields, &TableKey::keyword("value"))
            .unwrap()
            .is_nil());
        assert!(!sorted_struct_get(fields, &TableKey::keyword("error"))
            .unwrap()
            .is_nil());
    });
}

#[test]
fn test_wait_timeout_zero_returns_empty() {
    let backend = AsyncBackend::new().unwrap();
    let completions = backend.wait(0).unwrap();
    assert!(completions.is_empty());
}

/// A backend the program never let go of lets go of its own holds before the
/// heap that carries it tears its regions down.
///
/// The trap: `RegionStore::teardown_all` frees regions in id order, not
/// lifetime order. A backend reached only through the heap has its destructor
/// run from inside that sweep, so a release there names regions the same sweep
/// may already have freed. The fiber below lives in a region minted before the
/// backend's, which is what makes the order deterministic rather than a race
/// against id assignment.
///
/// Counter-factual: with the heap's pre-teardown drain removed, the release
/// runs from the destructor instead and trips the phantom-region assertion in
/// `RegionStore::decref_reaches_zero` — the shape `docs/concurrency.md`
/// reached at process exit on the thread-pool platform.
#[test]
fn a_stranded_backend_lets_go_before_its_heap_tears_down() {
    // An owned heap, not `TestHeap`: that one's heap is leaked for the process,
    // so it never reaches the teardown this test is about.
    let mut owned = Box::new(crate::value::fiberheap::FiberHeap::new());
    let heap: *mut crate::value::fiberheap::FiberHeap = &mut *owned;
    // SAFETY: the box outlives every use below, and nothing else names it.
    let h = unsafe { &mut *heap };
    let (fiber, _handle) =
        crate::value::fiber::test_fiber_in_region(h, crate::value::fiber::FiberStatus::Paused);

    // The thread-pool platform on every host: its `quiesce_pending` reaps
    // nothing, so the entry is still filed when the heap goes and the release
    // is the only thing that can let the fiber's region go. The ring would reap
    // the sleep at the drain and hide the question.
    let backend = AsyncBackend::new_thread_pool().unwrap();
    let req = IoRequest {
        op: IoOp::Sleep {
            duration: std::time::Duration::from_secs(30),
        },
        port: Value::NIL,
        timeout: None,
    };
    backend
        .submit(&req, crate::io::pending::Submitter::new(heap, fiber))
        .unwrap();
    assert!(backend.has_pending(), "the sleep must still be in flight");

    // Reachable only from the heap, and from a region minted after the fiber's:
    // this is the backend nobody dropped.
    let region = h.new_runtime_region();
    crate::value::build::external(
        h,
        "io-backend",
        crate::io::AnyBackend(Box::new(backend)),
        region,
    );

    drop(owned);
}
