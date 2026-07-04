//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::io::IoBackend;

#[test]
fn test_mock_read() {
    crate::value::arena::with_test_region(|| {
        let mock = MockBackend::new();
        mock.seed_read(b"hello world".to_vec());

        let req = IoRequest {
            op: IoOp::ReadAll,
            port: Value::NIL,
            timeout: None,
        };
        let id = mock
            .submit(&req, crate::value::arena::leaked_test_heap())
            .unwrap();
        assert_eq!(id.as_u64(), 1);

        let completions = mock.poll();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id.as_u64(), 1);
        assert!(completions[0].result.is_ok());
    });
}

#[test]
fn test_mock_write() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let mock = MockBackend::new();
        let req = IoRequest {
            op: IoOp::Write {
                data: h.ctx().string("test data"),
            },
            port: Value::NIL,
            timeout: None,
        };
        let id = mock
            .submit(&req, crate::value::arena::leaked_test_heap())
            .unwrap();
        let completions = mock.poll();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        let val = completions[0].result.as_ref().unwrap();
        assert_eq!(val.as_int(), Some(9));
    });
}

#[test]
fn test_mock_error_injection() {
    crate::value::arena::with_test_region(|| {
        let mock = MockBackend::new();
        mock.inject_error(5); // EIO

        let req = IoRequest {
            op: IoOp::ReadAll,
            port: Value::NIL,
            timeout: None,
        };
        mock.submit(&req, crate::value::arena::leaked_test_heap())
            .unwrap();

        let completions = mock.poll();
        assert_eq!(completions.len(), 1);
        assert!(completions[0].result.is_err());
    });
}

#[test]
fn test_mock_call_log() {
    crate::value::arena::with_test_region(|| {
        let mock = MockBackend::new();
        mock.seed_read(b"data".to_vec());

        let _ = mock.submit(
            &IoRequest {
                op: IoOp::ReadAll,
                port: Value::NIL,
                timeout: None,
            },
            crate::value::arena::leaked_test_heap(),
        );
        let _ = mock.submit(
            &IoRequest {
                op: IoOp::Flush,
                port: Value::NIL,
                timeout: None,
            },
            crate::value::arena::leaked_test_heap(),
        );

        let log = mock.take_log();
        assert_eq!(log, vec!["read-all", "flush"]);
    });
}

#[test]
fn test_mock_eof_no_data() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let mock = MockBackend::new();
        let req = IoRequest {
            op: IoOp::ReadLine {
                buffer: h.ctx().bytes(vec![0u8; 64]),
            },
            port: Value::NIL,
            timeout: None,
        };
        mock.submit(&req, crate::value::arena::leaked_test_heap())
            .unwrap();
        let completions = mock.poll();
        assert_eq!(completions.len(), 1);
        assert_eq!(*completions[0].result.as_ref().unwrap(), Value::NIL);
    });
}

#[test]
fn test_mock_monotonic_ids() {
    let mock = MockBackend::new();
    let id1 = mock
        .submit(
            &IoRequest {
                op: IoOp::Flush,
                port: Value::NIL,
                timeout: None,
            },
            crate::value::arena::leaked_test_heap(),
        )
        .unwrap();
    let id2 = mock
        .submit(
            &IoRequest {
                op: IoOp::Flush,
                port: Value::NIL,
                timeout: None,
            },
            crate::value::arena::leaked_test_heap(),
        )
        .unwrap();
    assert!(id2 > id1);
}

#[test]
fn test_mock_latency_poll_before_deadline() {
    let mock = MockBackend::new();
    mock.set_latency(Duration::from_millis(100));

    mock.submit(
        &IoRequest {
            op: IoOp::Flush,
            port: Value::NIL,
            timeout: None,
        },
        crate::value::arena::leaked_test_heap(),
    )
    .unwrap();

    // Poll immediately — should be empty (latency not elapsed)
    let completions = mock.poll();
    assert!(completions.is_empty());
}

#[test]
fn test_mock_latency_wait() {
    let mock = MockBackend::new();
    mock.set_latency(Duration::from_millis(10));

    mock.submit(
        &IoRequest {
            op: IoOp::Flush,
            port: Value::NIL,
            timeout: None,
        },
        crate::value::arena::leaked_test_heap(),
    )
    .unwrap();

    // Wait should sleep until deadline and return the completion
    let completions = mock.wait(-1).unwrap();
    assert_eq!(completions.len(), 1);
}

#[test]
fn test_mock_latency_wait_timeout() {
    let mock = MockBackend::new();
    mock.set_latency(Duration::from_secs(10)); // very long

    mock.submit(
        &IoRequest {
            op: IoOp::Flush,
            port: Value::NIL,
            timeout: None,
        },
        crate::value::arena::leaked_test_heap(),
    )
    .unwrap();

    // Wait with short timeout — should return empty
    let completions = mock.wait(5).unwrap();
    assert!(completions.is_empty());
}

#[test]
fn test_mock_cancel() {
    let mock = MockBackend::new();
    mock.set_latency(Duration::from_secs(10));

    let id = mock
        .submit(
            &IoRequest {
                op: IoOp::Flush,
                port: Value::NIL,
                timeout: None,
            },
            crate::value::arena::leaked_test_heap(),
        )
        .unwrap();

    mock.cancel(id).unwrap();

    // Nothing should be pending
    let completions = mock.wait(0).unwrap();
    assert!(completions.is_empty());
}

#[test]
fn test_mock_sleep_uses_duration() {
    let mock = MockBackend::new();
    // Default latency is zero, but Sleep should use its own duration
    let req = IoRequest {
        op: IoOp::Sleep {
            duration: Duration::from_millis(10),
        },
        port: Value::NIL,
        timeout: None,
    };
    mock.submit(&req, crate::value::arena::leaked_test_heap())
        .unwrap();

    // Poll immediately — Sleep's 10ms hasn't elapsed
    let completions = mock.poll();
    assert!(completions.is_empty());

    // Wait should return after the sleep duration
    let completions = mock.wait(-1).unwrap();
    assert_eq!(completions.len(), 1);
}
