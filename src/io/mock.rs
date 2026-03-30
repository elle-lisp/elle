//! Mock I/O backend for testing and benchmarking.
//!
//! Fulfills `IoRequest`s from in-memory state. No OS resources needed.
//! Completions resolve after a configurable latency (zero by default).

use crate::io::request::{IoOp, IoRequest};
use crate::io::Completion;
use crate::value::{error_val, Value};

use std::cell::RefCell;
use std::collections::BinaryHeap;
use std::time::{Duration, Instant};

/// In-memory I/O backend with configurable latency.
///
/// - `set_latency(dur)` — completions become available after `dur`
/// - `seed_read(data)` — pre-seed data for ReadLine/Read/ReadAll
/// - `inject_error(errno)` — make the next operation fail
/// - `take_log()` — retrieve and clear the operation log
pub(crate) struct MockBackend {
    inner: RefCell<MockInner>,
}

struct MockInner {
    next_id: u64,
    latency: Duration,
    pending: BinaryHeap<Pending>,
    read_data: Vec<Vec<u8>>,
    read_cursor: usize,
    error_queue: Vec<i32>,
    error_cursor: usize,
    log: Vec<String>,
}

/// A completion that becomes available at `deadline`.
struct Pending {
    deadline: Instant,
    completion: Completion,
}

// BinaryHeap is a max-heap; we want earliest deadline first.
impl Eq for Pending {}
impl PartialEq for Pending {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline
    }
}
impl Ord for Pending {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.deadline.cmp(&self.deadline) // reversed for min-heap
    }
}
impl PartialOrd for Pending {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl MockBackend {
    pub(crate) fn new() -> Self {
        MockBackend {
            inner: RefCell::new(MockInner {
                next_id: 1,
                latency: Duration::ZERO,
                pending: BinaryHeap::new(),
                read_data: Vec::new(),
                read_cursor: 0,
                error_queue: Vec::new(),
                error_cursor: 0,
                log: Vec::new(),
            }),
        }
    }

    /// Set the latency for future completions.
    #[allow(dead_code)]
    pub(crate) fn set_latency(&self, latency: Duration) {
        self.inner.borrow_mut().latency = latency;
    }

    /// Pre-seed read data. Each call adds one chunk that will be returned
    /// by the next ReadLine/Read/ReadAll operation.
    #[allow(dead_code)]
    pub(crate) fn seed_read(&self, data: Vec<u8>) {
        self.inner.borrow_mut().read_data.push(data);
    }

    /// Queue an error. The next operation will fail with the given errno.
    #[allow(dead_code)]
    pub(crate) fn inject_error(&self, errno: i32) {
        self.inner.borrow_mut().error_queue.push(errno);
    }

    /// Take the call log (clears it).
    #[allow(dead_code)]
    pub(crate) fn take_log(&self) -> Vec<String> {
        std::mem::take(&mut self.inner.borrow_mut().log)
    }
}

impl crate::io::IoBackend for MockBackend {
    fn submit(&self, request: &IoRequest) -> Result<u64, String> {
        let mut inner = self.inner.borrow_mut();
        let id = inner.next_id;
        inner.next_id += 1;

        let op_name = match &request.op {
            IoOp::ReadLine => "read-line",
            IoOp::Read { .. } => "read",
            IoOp::ReadAll => "read-all",
            IoOp::Write { .. } => "write",
            IoOp::Flush => "flush",
            IoOp::Accept => "accept",
            IoOp::Connect { .. } => "connect",
            IoOp::SendTo { .. } => "send-to",
            IoOp::RecvFrom { .. } => "recv-from",
            IoOp::Shutdown { .. } => "shutdown",
            IoOp::Sleep { .. } => "sleep",
            IoOp::Spawn(_) => "spawn",
            IoOp::ProcessWait => "process-wait",
            IoOp::Open { .. } => "open",
            IoOp::Seek { .. } => "seek",
            IoOp::Tell => "tell",
            IoOp::Task(_) => "task",
            IoOp::Resolve { .. } => "resolve",
            IoOp::WatchNext => "watch-next",
            IoOp::Close => "close",
            IoOp::PollFd { .. } => "poll-fd",
        };
        inner.log.push(op_name.to_string());

        // Check for injected error
        let result = if inner.error_cursor < inner.error_queue.len() {
            let errno = inner.error_queue[inner.error_cursor];
            inner.error_cursor += 1;
            Err(error_val(
                "io-error",
                format!("mock error: errno {}", errno),
            ))
        } else {
            match &request.op {
                IoOp::ReadLine | IoOp::Read { .. } | IoOp::ReadAll => {
                    if inner.read_cursor < inner.read_data.len() {
                        let data = inner.read_data[inner.read_cursor].clone();
                        inner.read_cursor += 1;
                        if data.is_empty() {
                            Ok(Value::NIL) // EOF
                        } else {
                            Ok(Value::string(String::from_utf8_lossy(&data).as_ref()))
                        }
                    } else {
                        Ok(Value::NIL) // EOF — no data seeded
                    }
                }
                IoOp::Write { data } => {
                    let len = data
                        .with_string(|s| s.len())
                        .or_else(|| data.as_bytes().map(|b| b.len()))
                        .unwrap_or(0);
                    Ok(Value::int(len as i64))
                }
                IoOp::Flush | IoOp::Shutdown { .. } => Ok(Value::NIL),
                IoOp::Sleep { duration } => {
                    // Sleep honors its own duration as latency override
                    let deadline = Instant::now() + *duration;
                    inner.pending.push(Pending {
                        deadline,
                        completion: Completion {
                            id,
                            result: Ok(Value::NIL),
                        },
                    });
                    return Ok(id);
                }
                IoOp::Accept => Err(error_val("io-error", "mock: accept not supported")),
                IoOp::Connect { .. } => Err(error_val("io-error", "mock: connect not supported")),
                IoOp::SendTo { data, .. } => {
                    let len = data
                        .with_string(|s| s.len())
                        .or_else(|| data.as_bytes().map(|b| b.len()))
                        .unwrap_or(0);
                    Ok(Value::int(len as i64))
                }
                IoOp::RecvFrom { .. } => {
                    if inner.read_cursor < inner.read_data.len() {
                        let data = inner.read_data[inner.read_cursor].clone();
                        inner.read_cursor += 1;
                        use crate::value::heap::TableKey;
                        let mut fields = std::collections::BTreeMap::new();
                        fields.insert(TableKey::Keyword("data".into()), Value::bytes(data));
                        fields.insert(TableKey::Keyword("addr".into()), Value::string("127.0.0.1"));
                        fields.insert(TableKey::Keyword("port".into()), Value::int(0));
                        Ok(Value::struct_from(fields))
                    } else {
                        Ok(Value::NIL)
                    }
                }
                IoOp::Spawn(_) | IoOp::ProcessWait => {
                    Err(error_val("io-error", "mock: subprocess ops not supported"))
                }
                IoOp::Open { .. } => Err(error_val("io-error", "mock: open not supported")),
                IoOp::Seek { .. } | IoOp::Tell => {
                    Err(error_val("io-error", "mock: seek/tell not supported"))
                }
                IoOp::Task(_) => Err(error_val("io-error", "mock: task not supported")),
                IoOp::Resolve { .. } => Err(error_val("io-error", "mock: resolve not supported")),
                IoOp::WatchNext => Err(error_val("io-error", "mock: watch not supported")),
                IoOp::PollFd { .. } => Err(error_val("io-error", "mock: poll-fd not supported")),
                // Close completes synchronously in submit
                IoOp::Close => Ok(Value::NIL),
            }
        };

        let deadline = Instant::now() + inner.latency;
        inner.pending.push(Pending {
            deadline,
            completion: Completion { id, result },
        });
        Ok(id)
    }

    fn poll(&self) -> Vec<Completion> {
        let mut inner = self.inner.borrow_mut();
        let now = Instant::now();
        let mut ready = Vec::new();
        while let Some(top) = inner.pending.peek() {
            if top.deadline <= now {
                ready.push(inner.pending.pop().unwrap().completion);
            } else {
                break;
            }
        }
        ready
    }

    fn wait(&self, timeout_ms: i64) -> Result<Vec<Completion>, String> {
        // Fast path: check for already-ready completions
        let ready = self.poll();
        if !ready.is_empty() {
            return Ok(ready);
        }

        // Nothing ready — sleep until the earliest deadline or timeout
        let inner = self.inner.borrow();
        let earliest = match inner.pending.peek() {
            Some(p) => p.deadline,
            None => return Ok(Vec::new()), // nothing pending at all
        };
        drop(inner);

        let now = Instant::now();
        let wait_until = if timeout_ms < 0 {
            earliest // wait forever → wait until earliest
        } else {
            let timeout_deadline = now + Duration::from_millis(timeout_ms as u64);
            earliest.min(timeout_deadline)
        };

        if wait_until > now {
            std::thread::sleep(wait_until - now);
        }

        Ok(self.poll())
    }

    fn cancel(&self, id: u64) -> Result<(), String> {
        let mut inner = self.inner.borrow_mut();
        // Remove the pending completion with this ID
        let old: Vec<Pending> = inner.pending.drain().collect();
        for p in old {
            if p.completion.id != id {
                inner.pending.push(p);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::IoBackend;

    #[test]
    fn test_mock_read() {
        let mock = MockBackend::new();
        mock.seed_read(b"hello world".to_vec());

        let req = IoRequest {
            op: IoOp::ReadAll,
            port: Value::NIL,
            timeout: None,
        };
        let id = mock.submit(&req).unwrap();
        assert_eq!(id, 1);

        let completions = mock.poll();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, 1);
        assert!(completions[0].result.is_ok());
    }

    #[test]
    fn test_mock_write() {
        let mock = MockBackend::new();
        let req = IoRequest {
            op: IoOp::Write {
                data: Value::string("test data"),
            },
            port: Value::NIL,
            timeout: None,
        };
        let id = mock.submit(&req).unwrap();
        let completions = mock.poll();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        let val = completions[0].result.as_ref().unwrap();
        assert_eq!(val.as_int(), Some(9));
    }

    #[test]
    fn test_mock_error_injection() {
        let mock = MockBackend::new();
        mock.inject_error(5); // EIO

        let req = IoRequest {
            op: IoOp::ReadAll,
            port: Value::NIL,
            timeout: None,
        };
        mock.submit(&req).unwrap();

        let completions = mock.poll();
        assert_eq!(completions.len(), 1);
        assert!(completions[0].result.is_err());
    }

    #[test]
    fn test_mock_call_log() {
        let mock = MockBackend::new();
        mock.seed_read(b"data".to_vec());

        let _ = mock.submit(&IoRequest {
            op: IoOp::ReadAll,
            port: Value::NIL,
            timeout: None,
        });
        let _ = mock.submit(&IoRequest {
            op: IoOp::Flush,
            port: Value::NIL,
            timeout: None,
        });

        let log = mock.take_log();
        assert_eq!(log, vec!["read-all", "flush"]);
    }

    #[test]
    fn test_mock_eof_no_data() {
        let mock = MockBackend::new();
        let req = IoRequest {
            op: IoOp::ReadLine,
            port: Value::NIL,
            timeout: None,
        };
        mock.submit(&req).unwrap();
        let completions = mock.poll();
        assert_eq!(completions.len(), 1);
        assert_eq!(*completions[0].result.as_ref().unwrap(), Value::NIL);
    }

    #[test]
    fn test_mock_monotonic_ids() {
        let mock = MockBackend::new();
        let id1 = mock
            .submit(&IoRequest {
                op: IoOp::Flush,
                port: Value::NIL,
                timeout: None,
            })
            .unwrap();
        let id2 = mock
            .submit(&IoRequest {
                op: IoOp::Flush,
                port: Value::NIL,
                timeout: None,
            })
            .unwrap();
        assert!(id2 > id1);
    }

    #[test]
    fn test_mock_latency_poll_before_deadline() {
        let mock = MockBackend::new();
        mock.set_latency(Duration::from_millis(100));

        mock.submit(&IoRequest {
            op: IoOp::Flush,
            port: Value::NIL,
            timeout: None,
        })
        .unwrap();

        // Poll immediately — should be empty (latency not elapsed)
        let completions = mock.poll();
        assert!(completions.is_empty());
    }

    #[test]
    fn test_mock_latency_wait() {
        let mock = MockBackend::new();
        mock.set_latency(Duration::from_millis(10));

        mock.submit(&IoRequest {
            op: IoOp::Flush,
            port: Value::NIL,
            timeout: None,
        })
        .unwrap();

        // Wait should sleep until deadline and return the completion
        let completions = mock.wait(-1).unwrap();
        assert_eq!(completions.len(), 1);
    }

    #[test]
    fn test_mock_latency_wait_timeout() {
        let mock = MockBackend::new();
        mock.set_latency(Duration::from_secs(10)); // very long

        mock.submit(&IoRequest {
            op: IoOp::Flush,
            port: Value::NIL,
            timeout: None,
        })
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
            .submit(&IoRequest {
                op: IoOp::Flush,
                port: Value::NIL,
                timeout: None,
            })
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
        mock.submit(&req).unwrap();

        // Poll immediately — Sleep's 10ms hasn't elapsed
        let completions = mock.poll();
        assert!(completions.is_empty());

        // Wait should return after the sleep duration
        let completions = mock.wait(-1).unwrap();
        assert_eq!(completions.len(), 1);
    }
}
