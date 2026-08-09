use super::*;
use libc;

#[test]
fn test_io_request_type_name() {
    crate::primitives::ctx::with_test_ctx(|ctx| {
        let buf = ctx.bytes(vec![0u8; 64]);
        let req = IoRequest::new(ctx, PortOp::ReadLine { buffer: buf }.into(), Value::NIL);
        assert_eq!(req.external_type_name(), Some("io-request"));
    });
}

#[test]
fn test_io_request_not_port() {
    crate::primitives::ctx::with_test_ctx(|ctx| {
        let req = IoRequest::new(ctx, PortOp::Flush.into(), Value::NIL);
        assert_ne!(req.external_type_name(), Some("port"));
    });
}

#[test]
fn test_io_request_with_timeout() {
    crate::primitives::ctx::with_test_ctx(|ctx| {
        let timeout = Some(Duration::from_millis(5000));
        let buf = ctx.bytes(vec![0u8; 64]);
        let req = IoRequest::with_timeout(
            ctx,
            PortOp::ReadLine { buffer: buf }.into(),
            Value::NIL,
            timeout,
        );
        let extracted = req.as_external::<IoRequest>().unwrap();
        assert_eq!(extracted.timeout, timeout);
    });
}

#[test]
fn test_stdio_disposition_derives() {
    // Smoke test that StdioDisposition is Copy + Clone + Debug
    let d = StdioDisposition::Pipe;
    let _ = d; // Copy
    let _ = format!("{:?}", d); // Debug
}

#[test]
fn test_process_handle_pid() {
    // Spawn `true`, verify pid() returns a nonzero value.
    // Resolved through PATH: the binary is /bin/true on Linux and
    // /usr/bin/true on macOS.
    use std::process::Command;
    let child = Command::new("true").spawn().unwrap();
    let pid = child.id();
    let handle = ProcessHandle::new(pid, child);
    assert_eq!(handle.pid(), pid);
    assert!(handle.pid() > 0);
}

#[test]
fn test_process_handle_drop_does_not_panic() {
    // Drop with a running child should not panic.
    use std::process::Command;
    let child = Command::new("true").spawn().unwrap();
    let pid = child.id();
    let handle = ProcessHandle::new(pid, child);
    drop(handle); // should not panic
}

#[test]
fn test_ioop_seek_variant_carries_offset_and_whence() {
    let op = IoOp::Seek {
        offset: 42,
        whence: libc::SEEK_END,
    };
    match op {
        IoOp::Seek { offset, whence } => {
            assert_eq!(offset, 42);
            assert_eq!(whence, libc::SEEK_END);
        }
        _ => panic!("expected Seek variant"),
    }
}

#[test]
fn test_ioop_tell_variant_is_unit() {
    let op = IoOp::Tell;
    assert!(matches!(op, IoOp::Tell));
}

// ── The submit path's copy into the pending entry ────────────────────────
//
// `AsyncBackend::submit` moves a request's op into its `PendingOp::Port`
// entry with `op.clone()`, and the completion path reads the op back out of
// that entry to find its buffer, its count and its accept port. A `Clone`
// that drops a field would strand the in-flight operation on a default,
// which is why the copy is pinned per field rather than by `Clone` deriving
// at all.

#[test]
fn cloning_a_read_keeps_its_count_and_buffer() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let buffer = h.ctx().bytes(vec![0u8; 32]);
        let op = PortOp::Read { count: 17, buffer };
        match op.clone() {
            PortOp::Read {
                count: cloned_count,
                buffer: cloned_buffer,
            } => {
                assert_eq!(cloned_count, 17, "the clone keeps the requested count");
                assert_eq!(
                    cloned_buffer.as_heap_ptr(),
                    buffer.as_heap_ptr(),
                    "the clone points at the same fiber-heap buffer, not a copy",
                );
            }
            other => panic!("clone changed the variant: {:?}", other),
        }
    });
}

#[test]
fn cloning_an_accept_keeps_its_options_encoding_and_port() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let accept_port = h.ctx().bytes(vec![0u8; 8]);
        let options = SocketOptions {
            sndbuf: Some(4096),
            ..Default::default()
        };
        let op = PortOp::Accept {
            options: options.clone(),
            encoding: crate::port::Encoding::Text,
            accept_port,
        };
        match op.clone() {
            PortOp::Accept {
                options: cloned_options,
                encoding: cloned_encoding,
                accept_port: cloned_port,
            } => {
                assert_eq!(
                    cloned_options.sndbuf, options.sndbuf,
                    "the clone keeps the socket options the caller asked for",
                );
                assert_eq!(
                    cloned_encoding,
                    crate::port::Encoding::Text,
                    "the clone keeps the accepted port's encoding",
                );
                assert_eq!(
                    cloned_port.as_heap_ptr(),
                    accept_port.as_heap_ptr(),
                    "the clone points at the pre-allocated accept port",
                );
            }
            other => panic!("clone changed the variant: {:?}", other),
        }
    });
}

#[test]
fn test_writeable_buffer_ptr_and_truncate() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let buffer = h.ctx().bytes(vec![0u8; 16]);
        assert_eq!(buffer.as_bytes().unwrap().len(), 16);

        unsafe {
            let (ptr, len) = writeable_buffer_ptr(&buffer);
            assert_eq!(len, 16);
            std::ptr::copy_nonoverlapping(b"hello world".as_ptr(), ptr, 11);
        }

        unsafe {
            truncate_buffer(&buffer, 5);
        }
        assert_eq!(buffer.as_bytes().unwrap().len(), 5);
        assert_eq!(buffer.as_bytes().unwrap(), b"hello");
    });
}

#[test]
fn test_bytes_to_string_in_place_valid_utf8() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let buffer = h.ctx().bytes(b"hello world".to_vec());
        unsafe {
            truncate_buffer(&buffer, 11);
        }

        let result = unsafe {
            bytes_to_string_in_place(buffer, h.heap() as *mut crate::value::fiberheap::FiberHeap)
        };
        assert!(result.is_ok(), "valid UTF-8 should succeed");
        let string_val = result.unwrap();
        assert_eq!(string_val.type_name(), "string");
        assert_eq!(
            string_val.with_string(|s| s.to_string()).unwrap(),
            "hello world"
        );
        assert_eq!(string_val.as_heap_ptr(), buffer.as_heap_ptr());
    });
}

#[test]
fn test_bytes_to_string_in_place_invalid_utf8() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let buffer = h.ctx().bytes(b"\xff\xfe".to_vec());
        unsafe {
            truncate_buffer(&buffer, 2);
        }

        let result = unsafe {
            bytes_to_string_in_place(buffer, h.heap() as *mut crate::value::fiberheap::FiberHeap)
        };
        assert!(result.is_err(), "invalid UTF-8 should fail");
        let err = result.unwrap_err();
        assert_eq!(err.type_name(), "struct");
    });
}

#[test]
fn test_bytes_to_string_in_place_empty() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let buffer = h.ctx().bytes(vec![]);
        let result = unsafe {
            bytes_to_string_in_place(buffer, h.heap() as *mut crate::value::fiberheap::FiberHeap)
        };
        assert!(result.is_ok(), "empty bytes should become empty string");
        let string_val = result.unwrap();
        assert_eq!(string_val.with_string(|s| s.len()).unwrap(), 0);
    });
}
