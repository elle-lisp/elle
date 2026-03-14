//! AsyncBackend — asynchronous I/O backend.
//!
//! Uses io_uring on Linux (feature-gated), thread-pool fallback elsewhere.
//! Wrapped as ExternalObject with type_name "io-backend" (same as SyncBackend).

use crate::io::completion;
use crate::io::pool::{BufferHandle, BufferPool};
use crate::io::request::{ConnectAddr, IoOp, IoRequest};
use crate::io::threadpool::{PoolCompletion, PoolOp, StdinOpKind, StdinThread, ThreadPoolBackend};
use crate::io::types::{FdState, PortKey};
use crate::port::{Encoding, Port, PortKind};
use crate::value::heap::TableKey;
use crate::value::{error_val, Value};

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::os::unix::io::RawFd;
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
///
/// Three variants matching the three port lifecycles:
/// - `Port`: operates on an existing port (stream I/O, accept, datagram, shutdown)
/// - `Connect`: creates a new port on completion (no existing port)
/// - `Sleep`: portless timer
pub(crate) enum PendingOp {
    /// Operation on an existing port.
    Port {
        op: IoOp,
        port_key: PortKey,
        port: Value,
        buffer_handle: BufferHandle,
        /// For Accept: which kind of listener (TcpListener or UnixListener).
        listener_kind: Option<PortKind>,
    },
    /// Connect to a remote address. Creates a new port on completion.
    Connect {
        addr: ConnectAddr,
        buffer_handle: BufferHandle,
        /// io_uring: pre-created socket fd. Thread pool: set to result fd
        /// on completion. Cleared on connect failure (fd closed).
        connect_fd: Option<RawFd>,
    },
    /// Async timer. No port.
    Sleep { buffer_handle: BufferHandle },
}

impl PendingOp {
    pub(crate) fn buffer_handle(&self) -> BufferHandle {
        match self {
            PendingOp::Port { buffer_handle, .. } => *buffer_handle,
            PendingOp::Connect { buffer_handle, .. } => *buffer_handle,
            PendingOp::Sleep { buffer_handle, .. } => *buffer_handle,
        }
    }
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
    #[cfg(target_os = "linux")]
    Uring(Box<io_uring::IoUring>),
    ThreadPool(ThreadPoolBackend),
}

/// High bit tag for timeout CQE user_data.
#[cfg(target_os = "linux")]
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

    #[cfg(target_os = "linux")]
    fn create_platform_backend() -> PlatformBackend {
        match io_uring::IoUring::new(256) {
            Ok(ring) => PlatformBackend::Uring(Box::new(ring)),
            Err(_) => PlatformBackend::ThreadPool(ThreadPoolBackend::new()),
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn create_platform_backend() -> PlatformBackend {
        PlatformBackend::ThreadPool(ThreadPoolBackend::new())
    }

    /// Submit an I/O request. Returns a submission ID.
    pub(crate) fn submit(&self, request: &IoRequest) -> Result<u64, String> {
        // Portless operations — handle before port extraction.
        if let IoOp::Connect { ref addr } = request.op {
            return self.submit_connect(addr, request.timeout);
        }
        if let IoOp::Sleep { duration } = request.op {
            return self.submit_sleep(duration);
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

        // Flush on socket ports is a no-op: fsync(2) returns EINVAL on sockets.
        // Return an immediate successful completion rather than submitting to the pool.
        if matches!(&request.op, IoOp::Flush)
            && matches!(
                port.kind(),
                PortKind::TcpStream | PortKind::UnixStream | PortKind::UdpSocket
            )
        {
            inner.buffer_pool.release(buf_handle);
            inner.completions.push_back(Completion {
                id,
                result: Ok(Value::NIL),
            });
            return Ok(id);
        }

        // ReadLine / Read: check per-fd buffer first.
        // When a previous raw libc::read returned more data than one line (common
        // with TCP), the excess is stored in fd_states[port_key].buffer.
        // Serve subsequent reads from the buffer before submitting to the pool.
        {
            let state = inner
                .fd_states
                .entry(port_key.clone())
                .or_insert_with(FdState::new);
            match &request.op {
                IoOp::ReadLine => {
                    if let Some(pos) = state.buffer.iter().position(|&b| b == b'\n') {
                        let line_bytes: Vec<u8> = state.buffer.drain(..=pos).collect();
                        let s = String::from_utf8_lossy(&line_bytes);
                        let trimmed = s.trim_end_matches('\n').trim_end_matches('\r');
                        inner.buffer_pool.release(buf_handle);
                        inner.completions.push_back(Completion {
                            id,
                            result: Ok(Value::string(trimmed)),
                        });
                        return Ok(id);
                    }
                }
                IoOp::Read { count } => {
                    if !state.buffer.is_empty() {
                        let n = (*count).min(state.buffer.len());
                        let chunk: Vec<u8> = state.buffer.drain(..n).collect();
                        let value = match port.encoding() {
                            Encoding::Text => {
                                Value::string(String::from_utf8_lossy(&chunk).as_ref())
                            }
                            Encoding::Binary => Value::bytes(chunk),
                        };
                        inner.buffer_pool.release(buf_handle);
                        inner.completions.push_back(Completion {
                            id,
                            result: Ok(value),
                        });
                        return Ok(id);
                    }
                }
                _ => {}
            }
        }

        // Dispatch by operation type
        match &request.op {
            IoOp::Accept => {
                let listener_kind = Some(port.kind());

                let AsyncBackendInner {
                    ref mut platform,
                    ref mut network_pool,
                    ref mut pending,
                    buffer_pool: _,
                    ..
                } = *inner;

                match platform {
                    #[cfg(target_os = "linux")]
                    PlatformBackend::Uring(ring) => {
                        crate::io::uring::submit_uring_accept(ring, id, fd, request.timeout)?;
                    }
                    PlatformBackend::ThreadPool(_) => {
                        network_pool.submit(id, PoolOp::Accept { fd })?;
                    }
                }

                pending.insert(
                    id,
                    PendingOp::Port {
                        op: IoOp::Accept,
                        port_key,
                        port: request.port,
                        buffer_handle: buf_handle,
                        listener_kind,
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

                let AsyncBackendInner {
                    ref mut platform,
                    ref mut network_pool,
                    ref mut pending,
                    ref mut buffer_pool,
                    ..
                } = *inner;

                match platform {
                    #[cfg(target_os = "linux")]
                    PlatformBackend::Uring(ring) => {
                        let payload = format!("{}:{}\0", addr, port_num).into_bytes();
                        let mut full_payload = payload;
                        full_payload.extend_from_slice(&bytes);
                        crate::io::uring::submit_uring_sendto(
                            ring,
                            id,
                            fd,
                            &full_payload,
                            request.timeout,
                            buffer_pool,
                        )?;
                    }
                    PlatformBackend::ThreadPool(_) => {
                        let _ = buffer_pool;
                        network_pool.submit(
                            id,
                            PoolOp::SendTo {
                                fd,
                                addr: addr.clone(),
                                port: *port_num,
                                data: bytes,
                            },
                        )?;
                    }
                }

                pending.insert(
                    id,
                    PendingOp::Port {
                        op: IoOp::SendTo {
                            addr: addr.clone(),
                            port_num: *port_num,
                            data: *data,
                        },
                        port_key,
                        port: request.port,
                        buffer_handle: buf_handle,
                        listener_kind: None,
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

                match platform {
                    #[cfg(target_os = "linux")]
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
                    PlatformBackend::ThreadPool(_) => {
                        let _ = buffer_pool;
                        network_pool.submit(id, PoolOp::RecvFrom { fd, size: *count })?;
                    }
                }

                pending.insert(
                    id,
                    PendingOp::Port {
                        op: IoOp::RecvFrom { count: *count },
                        port_key,
                        port: request.port,
                        buffer_handle: buf_handle,
                        listener_kind: None,
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

                match platform {
                    #[cfg(target_os = "linux")]
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
                    PlatformBackend::ThreadPool(_) => {
                        let _ = buffer_pool;
                        network_pool.submit(id, PoolOp::Shutdown { fd, how: *how })?;
                    }
                }

                pending.insert(
                    id,
                    PendingOp::Port {
                        op: IoOp::Shutdown { how: *how },
                        port_key,
                        port: request.port,
                        buffer_handle: buf_handle,
                        listener_kind: None,
                    },
                );
                Ok(id)
            }
            // Stream I/O ops (ReadLine, Read, ReadAll, Write, Flush)
            _ => {
                let AsyncBackendInner {
                    ref mut platform,
                    ref mut buffer_pool,
                    ref mut pending,
                    ..
                } = *inner;

                match platform {
                    #[cfg(target_os = "linux")]
                    PlatformBackend::Uring(ring) => {
                        crate::io::uring::submit_uring_stream(
                            ring,
                            id,
                            fd,
                            &request.op,
                            request.timeout,
                            buffer_pool,
                            buf_handle,
                        )?;
                    }
                    PlatformBackend::ThreadPool(pool) => {
                        let _ = buffer_pool;
                        let pool_op = match &request.op {
                            IoOp::ReadLine | IoOp::Read { .. } | IoOp::ReadAll => {
                                let size = match &request.op {
                                    IoOp::Read { count } => *count,
                                    IoOp::ReadLine | IoOp::ReadAll => 4096,
                                    _ => unreachable!(),
                                };
                                PoolOp::Read { fd, size }
                            }
                            IoOp::Write { data } => {
                                let bytes = Self::extract_write_bytes(data);
                                PoolOp::Write { fd, data: bytes }
                            }
                            IoOp::Flush => PoolOp::Flush { fd },
                            _ => unreachable!(),
                        };
                        pool.submit(id, pool_op)?;
                    }
                }

                pending.insert(
                    id,
                    PendingOp::Port {
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

        let AsyncBackendInner {
            ref mut platform,
            ref mut network_pool,
            ref mut pending,
            ref mut buffer_pool,
            ..
        } = *inner;

        let uring_fd = match platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ring) => {
                let fd = crate::io::uring::submit_uring_connect(
                    ring,
                    id,
                    addr,
                    timeout,
                    buffer_pool,
                    buf_handle,
                )?;
                Some(fd)
            }
            PlatformBackend::ThreadPool(_) => {
                let _ = buffer_pool;
                let pool_op = match addr {
                    ConnectAddr::Tcp { addr: host, port } => PoolOp::ConnectTcp {
                        addr: format!("{}:{}", host, port),
                    },
                    ConnectAddr::Unix { path } => PoolOp::ConnectUnix { path: path.clone() },
                };
                network_pool.submit(id, pool_op)?;
                None
            }
        };

        pending.insert(
            id,
            PendingOp::Connect {
                addr: match addr {
                    ConnectAddr::Tcp { addr: host, port } => ConnectAddr::Tcp {
                        addr: host.clone(),
                        port: *port,
                    },
                    ConnectAddr::Unix { path } => ConnectAddr::Unix { path: path.clone() },
                },
                buffer_handle: buf_handle,
                connect_fd: uring_fd,
            },
        );
        Ok(id)
    }

    /// Submit a Sleep operation. No port — just a timer.
    fn submit_sleep(&self, duration: Duration) -> Result<u64, String> {
        let mut inner = self.inner.borrow_mut();
        let id = inner.next_id;
        inner.next_id += 1;
        let buf_handle = inner.buffer_pool.alloc(0);

        let AsyncBackendInner {
            ref mut platform,
            ref mut network_pool,
            ref mut pending,
            ..
        } = *inner;

        match platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ring) => {
                crate::io::uring::submit_uring_sleep(ring, id, duration)?;
            }
            PlatformBackend::ThreadPool(_) => {
                let nanos = duration.as_nanos() as u64;
                network_pool.submit(id, PoolOp::Sleep { nanos })?;
            }
        }

        pending.insert(
            id,
            PendingOp::Sleep {
                buffer_handle: buf_handle,
            },
        );
        Ok(id)
    }

    /// Cancel a pending I/O operation by submission ID.
    ///
    /// For io_uring: submits IORING_OP_ASYNC_CANCEL. The original SQE will
    /// generate a CQE with result = -ECANCELED; the cancel SQE's CQE is
    /// tagged and skipped by drain_cqes.
    ///
    /// For thread pool: no-op (thread pool operations cannot be cancelled
    /// mid-flight; the scheduler removes the pending entry and the completion
    /// is discarded when it arrives).
    pub(crate) fn cancel(&self, id: u64) -> Result<(), String> {
        let mut inner = self.inner.borrow_mut();
        match inner.platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ref mut ring) => {
                crate::io::uring::submit_uring_cancel(ring, id)?;
            }
            PlatformBackend::ThreadPool(_) => {
                // Thread pool: just remove from pending. The thread will
                // complete eventually; the completion will be discarded
                // because the pending entry is gone.
                inner.pending.remove(&id);
            }
        }
        Ok(())
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
                ref mut network_pool,
                ref mut pending,
                ref mut buffer_pool,
                ref mut fd_states,
                ref mut completions,
                ..
            } = *inner;

            match platform {
                #[cfg(target_os = "linux")]
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
                    // If platform pool has in-flight ops, wait on it.
                    // If only network pool has in-flight ops, wait on network pool.
                    // If both have ops, use select across both receivers.
                    let raw_completions = if pool.has_in_flight() && !network_pool.has_in_flight() {
                        pool.wait(timeout)?
                    } else if !pool.has_in_flight() && network_pool.has_in_flight() {
                        network_pool.wait(timeout)?
                    } else if pool.has_in_flight() && network_pool.has_in_flight() {
                        // Both have in-flight ops: select across both receivers.
                        let timeout_dur = timeout.map(std::time::Duration::from_millis);
                        let mut results = Vec::new();
                        // Try non-blocking drain first
                        results.extend(pool.poll());
                        results.extend(network_pool.poll());
                        if results.is_empty() {
                            // Block waiting for either
                            crossbeam_channel::select! {
                                recv(pool.receiver()) -> msg => {
                                    if let Ok(item) = msg {
                                        pool.record_completion();
                                        results.push(item);
                                        // Drain any extras
                                        results.extend(pool.poll());
                                        results.extend(network_pool.poll());
                                    }
                                }
                                recv(network_pool.receiver()) -> msg => {
                                    if let Ok(item) = msg {
                                        network_pool.record_completion();
                                        results.push(item);
                                        // Drain any extras
                                        results.extend(pool.poll());
                                        results.extend(network_pool.poll());
                                    }
                                }
                                default(timeout_dur.unwrap_or(std::time::Duration::MAX)) => {}
                            }
                        }
                        results
                    } else {
                        // Neither has in-flight ops — nothing to wait for.
                        Vec::new()
                    };
                    for PoolCompletion {
                        id,
                        result_code,
                        data,
                    } in raw_completions
                    {
                        if let Some(pending_op) = pending.remove(&id) {
                            let buf_handle = pending_op.buffer_handle();
                            let c = completion::process_raw_completion(
                                id,
                                result_code,
                                data,
                                &pending_op,
                                fd_states,
                                buffer_pool,
                                buf_handle,
                            );
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

    pub(crate) fn extract_write_bytes(data: &Value) -> Vec<u8> {
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
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ring) => {
                crate::io::uring::drain_cqes(
                    ring,
                    &mut self.pending,
                    &mut self.buffer_pool,
                    &mut self.fd_states,
                    &mut self.completions,
                );
            }
            PlatformBackend::ThreadPool(pool) => {
                let raw = pool.poll();
                for PoolCompletion {
                    id,
                    result_code,
                    data,
                } in raw
                {
                    if let Some(pending) = self.pending.remove(&id) {
                        let buf_handle = pending.buffer_handle();
                        let c = completion::process_raw_completion(
                            id,
                            result_code,
                            data,
                            &pending,
                            &mut self.fd_states,
                            &mut self.buffer_pool,
                            buf_handle,
                        );
                        self.completions.push_back(c);
                    }
                }
            }
        }
        // Also drain network pool (Accept, Connect, SendTo, RecvFrom, Shutdown).
        self.drain_network_completions();
    }

    /// Drain completions from the network thread pool into self.completions.
    /// The network pool handles Accept, Connect, SendTo, RecvFrom, Shutdown.
    fn drain_network_completions(&mut self) {
        let raw = self.network_pool.poll();
        for PoolCompletion {
            id,
            result_code,
            data,
        } in raw
        {
            if let Some(mut pending) = self.pending.remove(&id) {
                // Thread pool Connect: result_code is the new fd.
                // Stash it in connect_fd so the completion handler finds it there.
                if let PendingOp::Connect {
                    ref mut connect_fd, ..
                } = pending
                {
                    if result_code > 0 {
                        *connect_fd = Some(result_code);
                    }
                }
                let buf_handle = pending.buffer_handle();
                let c = completion::process_raw_completion(
                    id,
                    result_code,
                    data,
                    &pending,
                    &mut self.fd_states,
                    &mut self.buffer_pool,
                    buf_handle,
                );
                self.completions.push_back(c);
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
            | IoOp::Shutdown { .. }
            | IoOp::Sleep { .. } => return Err("io/submit: unsupported operation on stdin".into()),
        };
        stdin_thread.submit(id, op_kind)?;
        // No buffer needed for stdin (thread manages its own)
        let buf_handle = self.buffer_pool.alloc(0);
        self.pending.insert(
            id,
            PendingOp::Port {
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
                        self.buffer_pool.release(pending.buffer_handle());
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

    #[test]
    fn test_accept_via_uring() {
        use std::os::unix::io::FromRawFd;

        // Create a TCP listener via libc
        let listener_fd = unsafe {
            let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_NONBLOCK, 0);
            assert!(fd >= 0, "socket() failed");

            let opt: libc::c_int = 1;
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_REUSEADDR,
                &opt as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );

            let mut addr: libc::sockaddr_in = std::mem::zeroed();
            addr.sin_family = libc::AF_INET as libc::sa_family_t;
            addr.sin_port = 0; // ephemeral port
            addr.sin_addr.s_addr = u32::from(std::net::Ipv4Addr::LOCALHOST).to_be();

            let ret = libc::bind(
                fd,
                &addr as *const _ as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            );
            assert_eq!(ret, 0, "bind() failed: {}", std::io::Error::last_os_error());

            let ret = libc::listen(fd, 128);
            assert_eq!(ret, 0, "listen() failed");

            fd
        };

        // Get the bound port
        let bound_port = unsafe {
            let mut addr: libc::sockaddr_in = std::mem::zeroed();
            let mut len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
            libc::getsockname(
                listener_fd,
                &mut addr as *mut _ as *mut libc::sockaddr,
                &mut len,
            );
            u16::from_be(addr.sin_port)
        };

        let listener_port = Value::external(
            "port",
            Port::new_tcp_listener(
                unsafe { std::os::unix::io::OwnedFd::from_raw_fd(listener_fd) },
                format!("127.0.0.1:{}", bound_port),
            ),
        );

        let backend = AsyncBackend::new().unwrap();

        // Submit Accept
        let accept_req = IoRequest {
            op: IoOp::Accept,
            port: listener_port,
            timeout: None,
        };
        let accept_id = backend.submit(&accept_req).unwrap();

        // Connect from a background thread
        let port_copy = bound_port;
        let handle = std::thread::spawn(move || {
            // Small delay to ensure accept is submitted
            std::thread::sleep(std::time::Duration::from_millis(10));
            let _stream = std::net::TcpStream::connect(format!("127.0.0.1:{}", port_copy)).unwrap();
        });

        // Wait for the accept completion
        let completions = backend.wait(5000).unwrap();
        assert_eq!(
            completions.len(),
            1,
            "expected 1 completion, got {}",
            completions.len()
        );
        assert_eq!(completions[0].id, accept_id);
        assert!(
            completions[0].result.is_ok(),
            "accept failed: {:?}",
            completions[0].result
        );

        // The result should be a port
        let accepted = completions[0].result.as_ref().unwrap();
        assert_eq!(
            accepted.external_type_name(),
            Some("port"),
            "expected a port value"
        );

        handle.join().unwrap();
    }

    #[test]
    fn test_connect_via_uring() {
        // Create a TCP listener via std
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let bound_addr = listener.local_addr().unwrap();

        // Accept from a background thread so we don't deadlock
        let handle = std::thread::spawn(move || {
            let _accepted = listener.accept().unwrap();
            // Keep the accepted connection alive until the test is done
            std::thread::sleep(std::time::Duration::from_secs(2));
        });

        let backend = AsyncBackend::new().unwrap();

        // Submit Connect
        let connect_req = IoRequest {
            op: IoOp::Connect {
                addr: crate::io::request::ConnectAddr::Tcp {
                    addr: "127.0.0.1".to_string(),
                    port: bound_addr.port(),
                },
            },
            port: Value::NIL,
            timeout: None,
        };
        let connect_id = backend.submit(&connect_req).unwrap();

        // Wait for the connect completion
        let completions = backend.wait(5000).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, connect_id);
        assert!(
            completions[0].result.is_ok(),
            "connect failed: {:?}",
            completions[0].result
        );

        let connected = completions[0].result.as_ref().unwrap();
        assert_eq!(connected.external_type_name(), Some("port"));

        handle.join().unwrap();
    }

    /// Accept + connect on the same io_uring ring — the scheduler scenario.
    /// One fiber does tcp/accept, another does tcp/connect, both SQEs on
    /// the same ring. Both completions must arrive.
    #[test]
    fn test_accept_and_connect_concurrent() {
        use std::os::unix::io::FromRawFd;

        // Create a non-blocking TCP listener via libc
        let listener_fd = unsafe {
            let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_NONBLOCK, 0);
            assert!(fd >= 0);
            let opt: libc::c_int = 1;
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_REUSEADDR,
                &opt as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );
            let mut addr: libc::sockaddr_in = std::mem::zeroed();
            addr.sin_family = libc::AF_INET as libc::sa_family_t;
            addr.sin_port = 0;
            addr.sin_addr.s_addr = u32::from(std::net::Ipv4Addr::LOCALHOST).to_be();
            libc::bind(
                fd,
                &addr as *const _ as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            );
            libc::listen(fd, 128);
            fd
        };

        let bound_port = unsafe {
            let mut addr: libc::sockaddr_in = std::mem::zeroed();
            let mut len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
            libc::getsockname(
                listener_fd,
                &mut addr as *mut _ as *mut libc::sockaddr,
                &mut len,
            );
            u16::from_be(addr.sin_port)
        };

        let listener_port = Value::external(
            "port",
            Port::new_tcp_listener(
                unsafe { std::os::unix::io::OwnedFd::from_raw_fd(listener_fd) },
                format!("127.0.0.1:{}", bound_port),
            ),
        );

        let backend = AsyncBackend::new().unwrap();

        let accept_id = backend
            .submit(&IoRequest {
                op: IoOp::Accept,
                port: listener_port,
                timeout: None,
            })
            .unwrap();

        let connect_id = backend
            .submit(&IoRequest {
                op: IoOp::Connect {
                    addr: crate::io::request::ConnectAddr::Tcp {
                        addr: "127.0.0.1".to_string(),
                        port: bound_port,
                    },
                },
                port: Value::NIL,
                timeout: None,
            })
            .unwrap();

        // Collect completions — may arrive in 1 or 2 wait calls.
        let mut all = Vec::new();
        for _ in 0..5 {
            let cs = backend.wait(2000).unwrap();
            all.extend(cs);
            if all.len() >= 2 {
                break;
            }
        }

        assert_eq!(all.len(), 2, "expected 2 completions, got {}", all.len());
        for c in &all {
            assert!(c.result.is_ok(), "id={} failed: {:?}", c.id, c.result);
        }
        let ids: Vec<u64> = all.iter().map(|c| c.id).collect();
        assert!(ids.contains(&accept_id), "missing accept");
        assert!(ids.contains(&connect_id), "missing connect");
    }
}
