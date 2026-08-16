use super::*;

#[test]
fn test_async_backend_new() {
    let backend = AsyncBackend::new();
    assert!(backend.is_ok());
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
            .submit(&req1, crate::value::arena::leaked_test_heap())
            .unwrap();
        let id2 = backend
            .submit(&req2, crate::value::arena::leaked_test_heap())
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
        let result = backend.submit(&req, crate::value::arena::leaked_test_heap());
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
            .submit(&req, crate::value::arena::leaked_test_heap())
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
            .submit(&req, crate::value::arena::leaked_test_heap())
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
            sorted_struct_get(fields, &TableKey::Keyword("id".into()))
                .unwrap()
                .as_int(),
            Some(42)
        );
        assert!(
            sorted_struct_get(fields, &TableKey::Keyword("error".into()))
                .unwrap()
                .is_nil()
        );
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
            sorted_struct_get(fields, &TableKey::Keyword("id".into()))
                .unwrap()
                .as_int(),
            Some(7)
        );
        assert!(
            sorted_struct_get(fields, &TableKey::Keyword("value".into()))
                .unwrap()
                .is_nil()
        );
        assert!(
            !sorted_struct_get(fields, &TableKey::Keyword("error".into()))
                .unwrap()
                .is_nil()
        );
    });
}

#[test]
fn test_wait_timeout_zero_returns_empty() {
    let backend = AsyncBackend::new().unwrap();
    let completions = backend.wait(0).unwrap();
    assert!(completions.is_empty());
}
