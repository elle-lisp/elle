//! Mock I/O backend for testing and benchmarking.
//!
//! Fulfills `IoRequest`s from in-memory state. No OS resources needed.
//! Completions resolve after a configurable latency (zero by default).

use crate::io::request::{IoOp, IoRequest, PortOp};
use crate::io::{Completion, SubmissionId};
use crate::value::Value;

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

impl MockInner {
    /// Mint the next unique, monotonically increasing submission id.
    fn mint_id(&mut self) -> SubmissionId {
        let id = SubmissionId::from_raw(self.next_id);
        self.next_id += 1;
        id
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
    fn submit(
        &self,
        request: &IoRequest,
        origin_heap: *mut crate::value::fiberheap::FiberHeap,
    ) -> Result<SubmissionId, String> {
        let mut inner = self.inner.borrow_mut();
        let id = inner.mint_id();

        let op_name = match &request.op {
            IoOp::Port(PortOp::ReadLine { .. }) => "read-line",
            IoOp::Port(PortOp::Read { .. }) => "read",
            IoOp::Port(PortOp::ReadExact { .. }) => "read-exact",
            IoOp::Port(PortOp::ReadAll) => "read-all",
            IoOp::Port(PortOp::Write { .. }) => "write",
            IoOp::Port(PortOp::Flush) => "flush",
            IoOp::Port(PortOp::Accept { .. }) => "accept",
            IoOp::Port(PortOp::SendTo { .. }) => "send-to",
            IoOp::Port(PortOp::RecvFrom { .. }) => "recv-from",
            IoOp::Port(PortOp::Shutdown { .. }) => "shutdown",
            IoOp::Connect { .. } => "connect",
            IoOp::Sleep { .. } => "sleep",
            IoOp::Spawn(_) => "spawn",
            IoOp::ProcessWait => "process-wait",
            IoOp::Open { .. } => "open",
            IoOp::Seek { .. } => "seek",
            IoOp::Tell => "tell",
            IoOp::Task(_) => "task",
            IoOp::Resolve { .. } => "resolve",
            IoOp::WatchNext => "watch-next",
            IoOp::SigNext => "sig-next",
            IoOp::Close => "close",
            IoOp::PollFd { .. } => "poll-fd",
            IoOp::ChanSelectPark(_) => "chan-select-park",
        };
        inner.log.push(op_name.to_string());

        // Check for injected error
        let result = if inner.error_cursor < inner.error_queue.len() {
            let errno = inner.error_queue[inner.error_cursor];
            inner.error_cursor += 1;
            Err(crate::io::io_error(
                "io-error",
                format!("mock error: errno {}", errno),
                origin_heap,
            ))
        } else {
            match &request.op {
                IoOp::Port(op) => match op {
                    PortOp::ReadLine { .. }
                    | PortOp::Read { .. }
                    | PortOp::ReadExact { .. }
                    | PortOp::ReadAll => {
                        if inner.read_cursor < inner.read_data.len() {
                            let data = inner.read_data[inner.read_cursor].clone();
                            inner.read_cursor += 1;
                            if data.is_empty() {
                                Ok(Value::NIL) // EOF
                            } else {
                                let heap =
                                    unsafe { &mut *crate::io::completion_heap_ptr(origin_heap) };
                                let ctx = crate::primitives::ctx::Alloc::new(heap);
                                Ok(ctx.string(String::from_utf8_lossy(&data).as_ref()))
                            }
                        } else {
                            Ok(Value::NIL) // EOF — no data seeded
                        }
                    }
                    PortOp::Write { data } => {
                        let len = data
                            .with_string(|s| s.len())
                            .or_else(|| data.as_bytes().map(|b| b.len()))
                            .unwrap_or(0);
                        Ok(Value::int(len as i64))
                    }
                    PortOp::Flush | PortOp::Shutdown { .. } => Ok(Value::NIL),
                    PortOp::Accept { .. } => Err(crate::io::io_error(
                        "io-error",
                        "mock: accept not supported",
                        origin_heap,
                    )),
                    PortOp::SendTo { data, .. } => {
                        let len = data
                            .with_string(|s| s.len())
                            .or_else(|| data.as_bytes().map(|b| b.len()))
                            .unwrap_or(0);
                        Ok(Value::int(len as i64))
                    }
                    PortOp::RecvFrom { result, .. } => {
                        if inner.read_cursor < inner.read_data.len() {
                            let payload = inner.read_data[inner.read_cursor].clone();
                            inner.read_cursor += 1;
                            // Fill the pre-allocated result struct (born on the
                            // requesting fiber's heap) in place — same discipline as
                            // the real backends, no fresh allocation here.
                            use crate::io::request::{
                                bytes_to_string_in_place, set_struct_field_in_place,
                                truncate_buffer, writeable_buffer_ptr,
                            };
                            use crate::value::heap::TableKey;
                            let struct_ref =
                                result.as_struct().expect("recv result must be a struct");
                            let data_buf = crate::value::sorted_struct_get(
                                struct_ref,
                                &TableKey::Keyword("data".into()),
                            )
                            .copied()
                            .expect("recv result must have :data");
                            let addr_buf = crate::value::sorted_struct_get(
                                struct_ref,
                                &TableKey::Keyword("addr".into()),
                            )
                            .copied()
                            .expect("recv result must have :addr");
                            unsafe {
                                let (dst, cap) = writeable_buffer_ptr(&data_buf);
                                let n = payload.len().min(cap);
                                std::ptr::copy_nonoverlapping(payload.as_ptr(), dst, n);
                                truncate_buffer(&data_buf, n);

                                let abytes = b"127.0.0.1";
                                let (dst, cap) = writeable_buffer_ptr(&addr_buf);
                                let n2 = abytes.len().min(cap);
                                std::ptr::copy_nonoverlapping(abytes.as_ptr(), dst, n2);
                                truncate_buffer(&addr_buf, n2);
                                let addr_val = bytes_to_string_in_place(addr_buf, origin_heap)
                                    .unwrap_or(addr_buf);
                                set_struct_field_in_place(
                                    result,
                                    &TableKey::Keyword("addr".into()),
                                    addr_val,
                                );
                                // :port stays 0 (mock).
                            }
                            Ok(*result)
                        } else {
                            Ok(Value::NIL)
                        }
                    }
                },
                IoOp::Sleep { duration } => {
                    // Sleep honors its own duration as latency override
                    let deadline = Instant::now() + *duration;
                    inner.pending.push(Pending {
                        deadline,
                        completion: Completion::ok(id, Value::NIL),
                    });
                    return Ok(id);
                }
                IoOp::Connect { .. } => Err(crate::io::io_error(
                    "io-error",
                    "mock: connect not supported",
                    origin_heap,
                )),
                IoOp::Spawn(_) | IoOp::ProcessWait => Err(crate::io::io_error(
                    "io-error",
                    "mock: subprocess ops not supported",
                    origin_heap,
                )),
                IoOp::Open { .. } => Err(crate::io::io_error(
                    "io-error",
                    "mock: open not supported",
                    origin_heap,
                )),
                IoOp::Seek { .. } | IoOp::Tell => Err(crate::io::io_error(
                    "io-error",
                    "mock: seek/tell not supported",
                    origin_heap,
                )),
                IoOp::Task(_) => Err(crate::io::io_error(
                    "io-error",
                    "mock: task not supported",
                    origin_heap,
                )),
                IoOp::Resolve { .. } => Err(crate::io::io_error(
                    "io-error",
                    "mock: resolve not supported",
                    origin_heap,
                )),
                IoOp::WatchNext => Err(crate::io::io_error(
                    "io-error",
                    "mock: watch not supported",
                    origin_heap,
                )),
                IoOp::SigNext => Err(crate::io::io_error(
                    "io-error",
                    "mock: sig-next not supported",
                    origin_heap,
                )),
                IoOp::PollFd { .. } => Err(crate::io::io_error(
                    "io-error",
                    "mock: poll-fd not supported",
                    origin_heap,
                )),
                IoOp::ChanSelectPark(_) => Err(crate::io::io_error(
                    "io-error",
                    "mock: chan/wait-ready not supported",
                    origin_heap,
                )),
                // Close completes synchronously in submit
                IoOp::Close => Ok(Value::NIL),
            }
        };

        let deadline = Instant::now() + inner.latency;
        inner.pending.push(Pending {
            deadline,
            completion: Completion::new(id, result),
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

    fn cancel(&self, id: SubmissionId) -> Result<(), String> {
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
mod tests;
