//! AsyncBackend — asynchronous I/O backend.
//!
//! Uses io_uring on Linux (feature-gated), thread-pool fallback elsewhere.
//! Wrapped as ExternalObject with type_name "io-backend" (same as SyncBackend).

use crate::io::completion;
use crate::io::pool::{BufferHandle, BufferPool};
use crate::io::request::{ConnectAddr, IoOp, IoRequest};
use crate::io::threadpool::{
    StdinOpKind, StdinThread, ThreadPoolBackend, TP_OP_ACCEPT, TP_OP_CONNECT_TCP,
    TP_OP_CONNECT_UNIX, TP_OP_FLUSH, TP_OP_READ, TP_OP_RECV_FROM, TP_OP_SEND_TO, TP_OP_SHUTDOWN,
    TP_OP_WRITE,
};
use crate::io::types::{FdState, FdStatus, PortKey};
use crate::port::{Encoding, Port, PortKind};
use crate::value::heap::TableKey;
use crate::value::{error_val, Value};

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::Duration;

/// Completion from an async I/O operation.
pub(crate) struct Completion {
    pub(crate) id: u64,
    pub(crate) result: Result<Value, Value>,
}

impl Completion {
    /// Convert to an Elle struct: {:id n :value v :error nil} or {:id n :value nil :error e}
    pub(crate) fn to_value(&self) -> Value {
        let mut fields = BTreeMap::new();
        fields.insert(TableKey::Keyword("id".into()), Value::int(self.id as i64));
        match &self.result {
            Ok(v) => {
                fields.insert(TableKey::Keyword("value".into()), *v);
                fields.insert(TableKey::Keyword("error".into()), Value::NIL);
            }
            Err(e) => {
                fields.insert(TableKey::Keyword("value".into()), Value::NIL);
                fields.insert(TableKey::Keyword("error".into()), *e);
            }
        }
        Value::struct_from(fields)
    }
}

/// Pending async I/O operation.
pub(crate) struct PendingOp {
    pub(crate) op: IoOp,
    pub(crate) port_key: PortKey,
    pub(crate) port: Value,
    pub(crate) buffer_handle: BufferHandle,
    /// For Accept: which kind of listener (TcpListener or UnixListener).
    /// Used on completion to create the right stream port type.
    pub(crate) listener_kind: Option<PortKind>,
    /// For Connect: the address being connected to.
    /// Used on completion to create the right port type.
    #[allow(dead_code)]
    pub(crate) connect_addr: Option<ConnectAddr>,
    /// Per-operation timeout from IoRequest.
    #[allow(dead_code)]
    pub(crate) timeout: Option<Duration>,
}

/// Async I/O backend. Wrapped as ExternalObject "io-backend".
pub struct AsyncBackend {
    inner: RefCell<AsyncBackendInner>,
}

struct AsyncBackendInner {
    fd_states: HashMap<PortKey, FdState>,
    pending: HashMap<u64, PendingOp>,
    completions: VecDeque<Completion>,
    next_id: u64,
    buffer_pool: BufferPool,
    stdin_thread: Option<StdinThread>,
    platform: PlatformBackend,
    /// Thread pool for network operations. Always available regardless
    /// of whether io_uring is the primary platform backend.
    network_pool: ThreadPoolBackend,
}

// --- Platform backend dispatch ---

pub(crate) enum PlatformBackend {
    #[cfg(all(target_os = "linux", feature = "io-uring"))]
    Uring(Box<io_uring::IoUring>),
    ThreadPool(ThreadPoolBackend),
}

/// High bit tag for timeout CQE user_data.
#[cfg(all(target_os = "linux", feature = "io-uring"))]
pub(crate) const TIMEOUT_USER_DATA_TAG: u64 = 1 << 63;

impl std::fmt::Debug for AsyncBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#<io-backend:async>")
    }
}

impl AsyncBackend {
    /// Create a new async backend.
    ///
    /// On Linux with the `io-uring` feature, attempts io_uring first.
    /// Falls back to thread-pool on failure or on non-Linux platforms.
    pub fn new() -> Result<Self, String> {
        let platform = Self::create_platform_backend();
        Ok(AsyncBackend {
            inner: RefCell::new(AsyncBackendInner {
                fd_states: HashMap::new(),
                pending: HashMap::new(),
                completions: VecDeque::new(),
                next_id: 1,
                buffer_pool: BufferPool::new(),
                stdin_thread: None,
                platform,
                network_pool: ThreadPoolBackend::new(),
            }),
        })
    }

    #[cfg(all(target_os = "linux", feature = "io-uring"))]
    fn create_platform_backend() -> PlatformBackend {
        match io_uring::IoUring::new(256) {
            Ok(ring) => PlatformBackend::Uring(Box::new(ring)),
            Err(_) => PlatformBackend::ThreadPool(ThreadPoolBackend::new()),
        }
    }

    #[cfg(not(all(target_os = "linux", feature = "io-uring")))]
    fn create_platform_backend() -> PlatformBackend {
        PlatformBackend::ThreadPool(ThreadPoolBackend::new())
    }

    /// Submit an I/O request. Returns a submission ID.
    pub(crate) fn submit(&self, request: &IoRequest) -> Result<u64, String> {
        // Handle Connect before port extraction — Connect creates a new port,
        // so request.port is Value::NIL.
        if let IoOp::Connect { ref addr } = request.op {
            return self.submit_connect(addr, request.timeout);
        }

        let port = request
            .port
            .as_external::<Port>()
            .ok_or_else(|| "io/submit: request contains non-port value".to_string())?;

        if port.is_closed() {
            return Err("io/submit: port is closed".into());
        }

        let mut inner = self.inner.borrow_mut();
        let id = inner.next_id;
        inner.next_id += 1;

        let port_key = PortKey::from_port(port);

        // For stdin, route to stdin thread
        if matches!(port_key, PortKey::Stdin) {
            return inner.submit_stdin(id, &request.op);
        }

        // Determine fd
        let fd = match &port_key {
            PortKey::Stdout => 1,
            PortKey::Stderr => 2,
            PortKey::Fd(raw) => *raw,
            PortKey::Stdin => unreachable!(),
        };

        let buf_handle = inner.buffer_pool.alloc(4096);

        // Dispatch by operation type
        match &request.op {
            IoOp::Accept => {
                let listener_kind = Some(port.kind());
                let (op_kind, write_data, read_size) = (TP_OP_ACCEPT, Vec::new(), 0usize);

                #[allow(unused_mut)]
                let AsyncBackendInner {
                    ref mut platform,
                    ref mut network_pool,
                    ref mut pending,
                    buffer_pool: _,
                    ..
                } = *inner;

                // Try io_uring first if available
                match platform {
                    #[cfg(all(target_os = "linux", feature = "io-uring"))]
                    PlatformBackend::Uring(ring) => {
                        crate::io::uring::submit_uring_accept(ring, id, fd, request.timeout)?;
                    }
                    _ => {
                        network_pool.submit(id, fd, op_kind, write_data, read_size)?;
                    }
                }

                pending.insert(
                    id,
                    PendingOp {
                        op: IoOp::Accept,
                        port_key,
                        port: request.port,
                        buffer_handle: buf_handle,
                        listener_kind,
                        connect_addr: None,
                        timeout: request.timeout,
                    },
                );
                Ok(id)
            }
            IoOp::SendTo {
                ref addr,
                port_num,
                ref data,
            } => {
                let bytes = Self::extract_write_bytes(data);
                let mut payload = format!("{}:{}\0", addr, port_num).into_bytes();
                payload.extend_from_slice(&bytes);

                #[allow(unused_mut)]
                let AsyncBackendInner {
                    ref mut platform,
                    ref mut network_pool,
                    ref mut pending,
                    ref mut buffer_pool,
                    ..
                } = *inner;

                // Try io_uring first if available
                match platform {
                    #[cfg(all(target_os = "linux", feature = "io-uring"))]
                    PlatformBackend::Uring(ring) => {
                        crate::io::uring::submit_uring_sendto(
                            ring,
                            id,
                            fd,
                            &payload,
                            request.timeout,
                            buffer_pool,
                        )?;
                    }
                    _ => {
                        let _ = buffer_pool; // Used only in io_uring path
                        network_pool.submit(id, fd, TP_OP_SEND_TO, payload, 0)?;
                    }
                }

                pending.insert(
                    id,
                    PendingOp {
                        op: IoOp::SendTo {
                            addr: addr.clone(),
                            port_num: *port_num,
                            data: *data,
                        },
                        port_key,
                        port: request.port,
                        buffer_handle: buf_handle,
                        listener_kind: None,
                        connect_addr: None,
                        timeout: request.timeout,
                    },
                );
                Ok(id)
            }
            IoOp::RecvFrom { count } => {
                let AsyncBackendInner {
                    ref mut platform,
                    ref mut network_pool,
                    ref mut pending,
                    ref mut buffer_pool,
                    ..
                } = *inner;

                // Try io_uring first if available
                match platform {
                    #[cfg(all(target_os = "linux", feature = "io-uring"))]
                    PlatformBackend::Uring(ring) => {
                        crate::io::uring::submit_uring_recvfrom(
                            ring,
                            id,
                            fd,
                            *count,
                            request.timeout,
                            buffer_pool,
                        )?;
                    }
                    _ => {
                        let _ = buffer_pool; // Use buffer_pool to avoid warning
                        network_pool.submit(id, fd, TP_OP_RECV_FROM, Vec::new(), *count)?;
                    }
                }

                pending.insert(
                    id,
                    PendingOp {
                        op: IoOp::RecvFrom { count: *count },
                        port_key,
                        port: request.port,
                        buffer_handle: buf_handle,
                        listener_kind: None,
                        connect_addr: None,
                        timeout: request.timeout,
                    },
                );
                Ok(id)
            }
            IoOp::Shutdown { how } => {
                let AsyncBackendInner {
                    ref mut platform,
                    ref mut network_pool,
                    ref mut pending,
                    ref mut buffer_pool,
                    ..
                } = *inner;

                // Try io_uring first if available
                match platform {
                    #[cfg(all(target_os = "linux", feature = "io-uring"))]
                    PlatformBackend::Uring(ring) => {
                        crate::io::uring::submit_uring_shutdown(
                            ring,
                            id,
                            fd,
                            *how,
                            request.timeout,
                            buffer_pool,
                        )?;
                    }
                    _ => {
                        let _ = buffer_pool; // Use buffer_pool to avoid warning
                        network_pool.submit(id, fd, TP_OP_SHUTDOWN, vec![*how as u8], 0)?;
                    }
                }

                pending.insert(
                    id,
                    PendingOp {
                        op: IoOp::Shutdown { how: *how },
                        port_key,
                        port: request.port,
                        buffer_handle: buf_handle,
                        listener_kind: None,
                        connect_addr: None,
                        timeout: request.timeout,
                    },
                );
                Ok(id)
            }
            // Stream I/O ops (ReadLine, Read, ReadAll, Write, Flush)
            _ => {
                let (op_kind, write_data) = match &request.op {
                    IoOp::ReadLine | IoOp::Read { .. } | IoOp::ReadAll => (TP_OP_READ, Vec::new()),
                    IoOp::Write { data } => {
                        let bytes = Self::extract_write_bytes(data);
                        (TP_OP_WRITE, bytes)
                    }
                    IoOp::Flush => (TP_OP_FLUSH, Vec::new()),
                    _ => unreachable!(),
                };

                let read_size = match &request.op {
                    IoOp::Read { count } => *count,
                    IoOp::ReadLine | IoOp::ReadAll => 4096,
                    _ => 0,
                };

                let AsyncBackendInner {
                    ref mut platform,
                    ref mut buffer_pool,
                    ref mut pending,
                    ..
                } = *inner;

                match platform {
                    #[cfg(all(target_os = "linux", feature = "io-uring"))]
                    PlatformBackend::Uring(ring) => {
                        crate::io::uring::submit_uring(
                            ring,
                            id,
                            fd,
                            op_kind,
                            &write_data,
                            read_size,
                            buffer_pool,
                            buf_handle,
                        )?;
                    }
                    PlatformBackend::ThreadPool(pool) => {
                        let _ = buffer_pool; // Used only in io_uring path
                        pool.submit(id, fd, op_kind, write_data, read_size)?;
                    }
                }

                pending.insert(
                    id,
                    PendingOp {
                        op: match &request.op {
                            IoOp::ReadLine => IoOp::ReadLine,
                            IoOp::Read { count } => IoOp::Read { count: *count },
                            IoOp::ReadAll => IoOp::ReadAll,
                            IoOp::Write { data } => IoOp::Write { data: *data },
                            IoOp::Flush => IoOp::Flush,
                            _ => unreachable!(),
                        },
                        port_key,
                        port: request.port,
                        buffer_handle: buf_handle,
                        listener_kind: None,
                        connect_addr: None,
                        timeout: request.timeout,
                    },
                );
                Ok(id)
            }
        }
    }

    /// Submit a Connect operation. Connect creates a new port, so
    /// request.port is Value::NIL — we handle it separately.
    fn submit_connect(&self, addr: &ConnectAddr, timeout: Option<Duration>) -> Result<u64, String> {
        let mut inner = self.inner.borrow_mut();
        let id = inner.next_id;
        inner.next_id += 1;
        let buf_handle = inner.buffer_pool.alloc(0);

        let (op_kind, data) = match addr {
            ConnectAddr::Tcp { addr: host, port } => {
                (TP_OP_CONNECT_TCP, format!("{}:{}", host, port).into_bytes())
            }
            ConnectAddr::Unix { path } => (TP_OP_CONNECT_UNIX, path.as_bytes().to_vec()),
        };

        let AsyncBackendInner {
            ref mut platform,
            ref mut network_pool,
            ref mut pending,
            ref mut buffer_pool,
            ..
        } = *inner;

        // Try io_uring first if available
        match platform {
            #[cfg(all(target_os = "linux", feature = "io-uring"))]
            PlatformBackend::Uring(ring) => {
                crate::io::uring::submit_uring_connect(ring, id, addr, timeout, buffer_pool)?;
            }
            _ => {
                let _ = buffer_pool; // Use buffer_pool to avoid warning
                network_pool.submit(id, -1, op_kind, data, 0)?;
            }
        }

        pending.insert(
            id,
            PendingOp {
                op: IoOp::Connect {
                    addr: match addr {
                        ConnectAddr::Tcp { addr: host, port } => ConnectAddr::Tcp {
                            addr: host.clone(),
                            port: *port,
                        },
                        ConnectAddr::Unix { path } => ConnectAddr::Unix { path: path.clone() },
                    },
                },
                port_key: PortKey::Fd(-1),
                port: Value::NIL,
                buffer_handle: buf_handle,
                listener_kind: None,
                connect_addr: Some(match addr {
                    ConnectAddr::Tcp { addr: host, port } => ConnectAddr::Tcp {
                        addr: host.clone(),
                        port: *port,
                    },
                    ConnectAddr::Unix { path } => ConnectAddr::Unix { path: path.clone() },
                }),
                timeout,
            },
        );
        Ok(id)
    }

    /// Non-blocking poll for completions.
    pub(crate) fn poll(&self) -> Vec<Completion> {
        let mut inner = self.inner.borrow_mut();
        inner.drain_platform_completions();
        inner.drain_stdin_completions();
        inner.completions.drain(..).collect()
    }

    /// Blocking wait for completions.
    /// `timeout_ms`: negative = wait forever, 0 = poll, positive = wait up to N ms.
    pub(crate) fn wait(&self, timeout_ms: i64) -> Result<Vec<Completion>, String> {
        let mut inner = self.inner.borrow_mut();

        // First drain any buffered completions
        inner.drain_platform_completions();
        inner.drain_stdin_completions();
        if !inner.completions.is_empty() {
            return Ok(inner.completions.drain(..).collect());
        }

        // Nothing buffered — block on platform
        let timeout = if timeout_ms < 0 {
            None
        } else {
            Some(timeout_ms as u64)
        };

        // Destructure to get independent borrows of each field.
        // Scoped so the borrows end before drain_stdin_completions.
        {
            let AsyncBackendInner {
                ref mut platform,
                ref mut pending,
                ref mut buffer_pool,
                ref mut fd_states,
                ref mut completions,
                ..
            } = *inner;

            match platform {
                #[cfg(all(target_os = "linux", feature = "io-uring"))]
                PlatformBackend::Uring(ring) => {
                    crate::io::uring::wait_uring(
                        ring,
                        timeout,
                        pending,
                        buffer_pool,
                        fd_states,
                        completions,
                    )?;
                }
                PlatformBackend::ThreadPool(pool) => {
                    let raw_completions = pool.wait(timeout)?;
                    for (id, result_code, data) in raw_completions {
                        if let Some(pending_op) = pending.remove(&id) {
                            let buf_handle = pending_op.buffer_handle;
                            // Release buffer first
                            buffer_pool.release(buf_handle);

                            // Process completion
                            let c = if result_code < 0 {
                                let errno = -result_code;
                                let msg = format!("I/O error: errno {}", errno);
                                let state = fd_states
                                    .entry(pending_op.port_key.clone())
                                    .or_insert_with(FdState::new);
                                state.status = FdStatus::Error(msg.clone());
                                Completion {
                                    id,
                                    result: Err(error_val("io-error", msg)),
                                }
                            } else if result_code == 0
                                && matches!(
                                    pending_op.op,
                                    IoOp::ReadLine | IoOp::Read { .. } | IoOp::ReadAll
                                )
                            {
                                let state = fd_states
                                    .entry(pending_op.port_key.clone())
                                    .or_insert_with(FdState::new);
                                state.status = FdStatus::Eof;
                                Completion {
                                    id,
                                    result: Ok(Value::NIL),
                                }
                            } else {
                                let value = match &pending_op.op {
                                    IoOp::ReadLine => {
                                        let s = String::from_utf8_lossy(&data);
                                        let trimmed =
                                            s.trim_end_matches('\n').trim_end_matches('\r');
                                        Value::string(trimmed)
                                    }
                                    IoOp::Read { .. } | IoOp::ReadAll => {
                                        if let Some(port) = pending_op.port.as_external::<Port>() {
                                            match port.encoding() {
                                                Encoding::Text => {
                                                    let s = String::from_utf8_lossy(&data);
                                                    Value::string(s.as_ref())
                                                }
                                                Encoding::Binary => Value::bytes(data),
                                            }
                                        } else {
                                            Value::string(String::from_utf8_lossy(&data).as_ref())
                                        }
                                    }
                                    IoOp::Write { .. } | IoOp::SendTo { .. } => {
                                        Value::int(result_code as i64)
                                    }
                                    IoOp::Flush | IoOp::Shutdown { .. } => Value::NIL,
                                    IoOp::RecvFrom { .. } => {
                                        // Raw bytes from recvfrom — placeholder
                                        Value::bytes(data)
                                    }
                                    IoOp::Accept | IoOp::Connect { .. } => {
                                        // Network ops not yet routed through async
                                        Value::NIL
                                    }
                                };
                                Completion {
                                    id,
                                    result: Ok(value),
                                }
                            };
                            completions.push_back(c);
                        }
                    }
                }
            }
        }

        // Also drain stdin completions
        inner.drain_stdin_completions();

        Ok(inner.completions.drain(..).collect())
    }

    fn extract_write_bytes(data: &Value) -> Vec<u8> {
        if let Some(s) = data.with_string(|s| s.as_bytes().to_vec()) {
            s
        } else if let Some(b) = data.as_bytes() {
            b.to_vec()
        } else if let Some(b) = data.as_bytes_mut() {
            b.borrow().clone()
        } else if let Some(b) = data.as_string_mut() {
            b.borrow().clone()
        } else {
            format!("{}", data).into_bytes()
        }
    }

    /// Check if there are pending operations.
    /// Used by the async scheduler (Chunk 6) to determine when to exit the event loop.
    #[allow(dead_code)]
    pub(crate) fn has_pending(&self) -> bool {
        let inner = self.inner.borrow();
        !inner.pending.is_empty()
    }
}

impl AsyncBackendInner {
    /// Drain completions from the platform backend into self.completions.
    fn drain_platform_completions(&mut self) {
        match &mut self.platform {
            #[cfg(all(target_os = "linux", feature = "io-uring"))]
            PlatformBackend::Uring(ring) => {
                for cqe in ring.completion() {
                    let user_data = cqe.user_data();
                    let result_code = cqe.result();

                    // Skip timeout CQEs (they have the high bit set)
                    if user_data & TIMEOUT_USER_DATA_TAG != 0 {
                        continue;
                    }

                    let id = user_data;
                    if let Some(pending) = self.pending.remove(&id) {
                        let data = if result_code > 0 {
                            let buf = self.buffer_pool.get_mut(pending.buffer_handle);
                            buf[..result_code as usize].to_vec()
                        } else {
                            Vec::new()
                        };
                        let c = completion::process_raw_completion(
                            id,
                            result_code,
                            data,
                            &pending,
                            &mut self.fd_states,
                            &mut self.buffer_pool,
                            pending.buffer_handle,
                        );
                        self.completions.push_back(c);
                    }
                }
            }
            PlatformBackend::ThreadPool(pool) => {
                let raw = pool.poll();
                for (id, result_code, data) in raw {
                    if let Some(pending) = self.pending.remove(&id) {
                        let c = completion::process_raw_completion(
                            id,
                            result_code,
                            data,
                            &pending,
                            &mut self.fd_states,
                            &mut self.buffer_pool,
                            pending.buffer_handle,
                        );
                        self.completions.push_back(c);
                    }
                }
            }
        }
    }

    /// Submit a stdin operation.
    fn submit_stdin(&mut self, id: u64, op: &IoOp) -> Result<u64, String> {
        let stdin_thread = self.stdin_thread.get_or_insert_with(StdinThread::new);
        let op_kind = match op {
            IoOp::ReadLine => StdinOpKind::ReadLine,
            IoOp::Read { count } => StdinOpKind::Read { count: *count },
            IoOp::ReadAll => StdinOpKind::ReadAll,
            IoOp::Write { .. }
            | IoOp::Flush
            | IoOp::Accept
            | IoOp::Connect { .. }
            | IoOp::SendTo { .. }
            | IoOp::RecvFrom { .. }
            | IoOp::Shutdown { .. } => {
                return Err("io/submit: unsupported operation on stdin".into())
            }
        };
        stdin_thread.submit(id, op_kind)?;
        // No buffer needed for stdin (thread manages its own)
        let buf_handle = self.buffer_pool.alloc(0);
        self.pending.insert(
            id,
            PendingOp {
                op: match op {
                    IoOp::ReadLine => IoOp::ReadLine,
                    IoOp::Read { count } => IoOp::Read { count: *count },
                    IoOp::ReadAll => IoOp::ReadAll,
                    _ => unreachable!(),
                },
                port_key: PortKey::Stdin,
                port: Value::NIL, // stdin has no port Value
                buffer_handle: buf_handle,
                listener_kind: None,
                connect_addr: None,
                timeout: None,
            },
        );
        Ok(id)
    }

    /// Drain stdin completions.
    fn drain_stdin_completions(&mut self) {
        let completions_to_add: Vec<Completion> = if let Some(ref stdin_thread) = self.stdin_thread
        {
            stdin_thread
                .poll_completions()
                .into_iter()
                .filter_map(|sc| {
                    if let Some(pending) = self.pending.remove(&sc.id) {
                        self.buffer_pool.release(pending.buffer_handle);
                        let c = match sc.result {
                            Ok(data) if data.is_empty() => Completion {
                                id: sc.id,
                                result: Ok(Value::NIL), // EOF
                            },
                            Ok(data) => Completion {
                                id: sc.id,
                                result: Ok(Value::string(String::from_utf8_lossy(&data).as_ref())),
                            },
                            Err(msg) => Completion {
                                id: sc.id,
                                result: Err(error_val("io-error", msg)),
                            },
                        };
                        Some(c)
                    } else {
                        None // Cancelled — discard
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        for c in completions_to_add {
            self.completions.push_back(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::request::{IoOp, IoRequest};
    use crate::port::{Direction, Encoding, Port};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn write_temp_file(content: &str) -> String {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = format!("/tmp/elle-test-async-{}-{}", std::process::id(), n);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn open_read_port(path: &str) -> Value {
        let file = std::fs::File::open(path).unwrap();
        let fd: std::os::unix::io::OwnedFd = file.into();
        Value::external(
            "port",
            Port::new_file(fd, Direction::Read, Encoding::Text, path.to_string()),
        )
    }

    fn open_write_port(path: &str) -> Value {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .unwrap();
        let fd: std::os::unix::io::OwnedFd = file.into();
        Value::external(
            "port",
            Port::new_file(fd, Direction::Write, Encoding::Text, path.to_string()),
        )
    }

    #[test]
    fn test_async_backend_new() {
        let backend = AsyncBackend::new();
        assert!(backend.is_ok());
    }

    #[test]
    fn test_submit_returns_monotonic_ids() {
        let backend = AsyncBackend::new().unwrap();
        let path = write_temp_file("hello");
        let port = open_read_port(&path);

        let req1 = IoRequest {
            op: IoOp::ReadAll,
            port,
            timeout: None,
        };
        let req2 = IoRequest {
            op: IoOp::ReadAll,
            port,
            timeout: None,
        };

        let id1 = backend.submit(&req1).unwrap();
        let id2 = backend.submit(&req2).unwrap();
        assert!(id2 > id1, "IDs must be monotonically increasing");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_submit_closed_port_errors() {
        let backend = AsyncBackend::new().unwrap();
        let path = write_temp_file("test");
        let port_val = open_read_port(&path);
        let port = port_val.as_external::<Port>().unwrap();
        port.close();

        let req = IoRequest {
            op: IoOp::ReadAll,
            port: port_val,
            timeout: None,
        };
        let result = backend.submit(&req);
        assert!(result.is_err());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_poll_empty_when_no_completions() {
        let backend = AsyncBackend::new().unwrap();
        let completions = backend.poll();
        assert!(completions.is_empty());
    }

    #[test]
    fn test_submit_and_wait_read() {
        let backend = AsyncBackend::new().unwrap();
        let path = write_temp_file("async read test");
        let port = open_read_port(&path);

        let req = IoRequest {
            op: IoOp::ReadAll,
            port,
            timeout: None,
        };
        let id = backend.submit(&req).unwrap();

        let completions = backend.wait(-1).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        assert!(completions[0].result.is_ok());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_submit_and_wait_write() {
        let backend = AsyncBackend::new().unwrap();
        let path = format!("/tmp/elle-test-async-write-{}", std::process::id());
        let port = open_write_port(&path);

        let req = IoRequest {
            op: IoOp::Write {
                data: Value::string("async write"),
            },
            port,
            timeout: None,
        };
        let id = backend.submit(&req).unwrap();

        let completions = backend.wait(-1).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        assert!(completions[0].result.is_ok());

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "async write");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_completion_to_value_success() {
        let c = Completion {
            id: 42,
            result: Ok(Value::string("hello")),
        };
        let v = c.to_value();
        let fields = v.as_struct().unwrap();
        assert_eq!(
            fields
                .get(&TableKey::Keyword("id".into()))
                .unwrap()
                .as_int(),
            Some(42)
        );
        assert!(fields
            .get(&TableKey::Keyword("error".into()))
            .unwrap()
            .is_nil());
    }

    #[test]
    fn test_completion_to_value_error() {
        let c = Completion {
            id: 7,
            result: Err(error_val("io-error", "test error")),
        };
        let v = c.to_value();
        let fields = v.as_struct().unwrap();
        assert_eq!(
            fields
                .get(&TableKey::Keyword("id".into()))
                .unwrap()
                .as_int(),
            Some(7)
        );
        assert!(fields
            .get(&TableKey::Keyword("value".into()))
            .unwrap()
            .is_nil());
        assert!(!fields
            .get(&TableKey::Keyword("error".into()))
            .unwrap()
            .is_nil());
    }

    #[test]
    fn test_wait_timeout_zero_returns_empty() {
        let backend = AsyncBackend::new().unwrap();
        let completions = backend.wait(0).unwrap();
        assert!(completions.is_empty());
    }
}
