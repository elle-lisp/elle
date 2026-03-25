//! AsyncBackend — asynchronous I/O backend.
//!
//! Uses io_uring on Linux (feature-gated), thread-pool fallback elsewhere.

use crate::io::completion;
use crate::io::pending::PendingOp;
use crate::io::pool::BufferPool;
use crate::io::request::{
    ConnectAddr, IoOp, IoRequest, ProcessHandle, ProcessState, SpawnRequest, StdioDisposition,
    TaskFn,
};
use crate::io::threadpool::{PoolCompletion, PoolOp, StdinOpKind, StdinThread, ThreadPoolBackend};
use crate::io::types::{FdState, PortKey};
use crate::io::Completion;
use crate::port::{Direction, Encoding, Port, PortKind};
use crate::value::{error_val, Value};

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::io;
use std::os::unix::io::AsRawFd;
use std::time::Duration;

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

        // Subprocess ops: portless (Spawn) or ProcessHandle-in-port (ProcessWait).
        if let IoOp::Spawn(ref req) = request.op {
            return self.submit_spawn(req);
        }
        if let IoOp::ProcessWait = request.op {
            return self.submit_process_wait(&request.port);
        }

        // Resolve is portless — always goes to the thread pool.
        if let IoOp::Resolve { ref hostname } = request.op {
            return self.submit_resolve(hostname);
        }

        // Open is portless — creates a new port rather than operating on one.
        if let IoOp::Open {
            ref path,
            flags,
            mode,
            direction,
            encoding,
        } = request.op
        {
            return self.submit_open(path, flags, mode, direction, encoding, request.timeout);
        }

        // Task: run closure on thread pool.
        if let IoOp::Task(ref task_fn) = request.op {
            return self.submit_task(task_fn);
        }

        let port = request
            .port
            .as_external::<Port>()
            .ok_or_else(|| "io/submit: request contains non-port value".to_string())?;

        // Close: cancel pending ops on this fd, then close the port.
        // Must come before the is_closed() check since the port is open
        // when close is requested.
        if matches!(&request.op, IoOp::Close) {
            let port_key = PortKey::from_port(port);
            if let PortKey::Fd(fd) = &port_key {
                let mut inner = self.inner.borrow_mut();
                // Cancel all pending ops on this fd
                let ids_to_cancel: Vec<u64> = inner
                    .pending
                    .iter()
                    .filter_map(|(&op_id, op)| match op {
                        PendingOp::Port { port_key: pk, .. } if *pk == port_key => Some(op_id),
                        _ => None,
                    })
                    .collect();

                for op_id in ids_to_cancel {
                    match inner.platform {
                        #[cfg(target_os = "linux")]
                        PlatformBackend::Uring(ref mut ring) => {
                            let _ = crate::io::uring::submit_uring_cancel(ring, op_id);
                        }
                        PlatformBackend::ThreadPool(_) => {
                            // Thread pool: remove pending entry. The blocking
                            // syscall will get EBADF when the fd closes.
                            inner.pending.remove(&op_id);
                        }
                    }
                }

                // Remove fd state
                inner.fd_states.remove(&PortKey::Fd(*fd));

                drop(inner);
            }

            // Now actually close the port (drops the fd).
            port.close();

            // Queue immediate completion.
            let mut inner = self.inner.borrow_mut();
            let id = inner.next_id;
            inner.next_id += 1;
            inner.completions.push_back(Completion {
                id,
                result: Ok(Value::NIL),
            });
            return Ok(id);
        }

        if port.is_closed() {
            return Err("io/submit: port is closed".into());
        }

        let mut inner = self.inner.borrow_mut();
        let id = inner.next_id;
        inner.next_id += 1;

        let port_key = PortKey::from_port(port);

        // Seek and Tell: synchronous file-only ops — handle as immediate completions.
        // Must come before stdin routing and buffer allocation.
        if matches!(&request.op, IoOp::Seek { .. } | IoOp::Tell) {
            return inner.handle_seek_tell(id, port, &port_key, &request.op);
        }

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

        // Flush on socket/pipe/stdio ports is a no-op: fsync(2) returns EINVAL on
        // non-file fds (sockets, pipes, and stdio when redirected to pipes in subprocesses).
        // Return an immediate successful completion rather than submitting to the pool.
        if matches!(&request.op, IoOp::Flush)
            && matches!(
                port.kind(),
                PortKind::TcpStream
                    | PortKind::UnixStream
                    | PortKind::UdpSocket
                    | PortKind::Pipe
                    | PortKind::Stdout
                    | PortKind::Stderr
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
        //
        // `read_buffered` tracks how many bytes were already in the buffer
        // when a Read request can't be fully served — the completion handler
        // must prepend those bytes to the fd data.
        let mut read_buffered: usize = 0;
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
                    if state.buffer.len() >= *count {
                        // Buffer has enough — serve entirely from buffer.
                        let chunk: Vec<u8> = state.buffer.drain(..*count).collect();
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
                    // Buffer has partial data — leave it in place and submit
                    // a read for the remaining bytes. The completion handler
                    // will prepend the buffered bytes.
                    read_buffered = state.buffer.len();
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
                            read_buffered,
                        )?;
                    }
                    PlatformBackend::ThreadPool(pool) => {
                        let _ = buffer_pool;
                        let pool_op = match &request.op {
                            IoOp::ReadLine => PoolOp::ReadLine { fd },
                            IoOp::Read { .. } | IoOp::ReadAll => {
                                let size = match &request.op {
                                    IoOp::Read { count } => *count - read_buffered,
                                    IoOp::ReadAll => 4096,
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
    fn submit_connect(
        &self,
        addr: &ConnectAddr,
        _timeout: Option<Duration>,
    ) -> Result<u64, String> {
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
                match crate::io::uring::submit_uring_connect(
                    ring,
                    id,
                    addr,
                    _timeout,
                    buffer_pool,
                    buf_handle,
                ) {
                    Ok(fd) => Some(fd),
                    Err(_) => {
                        // io_uring connect requires a parsed IP address.
                        // If parsing failed (hostname), fall back to thread pool
                        // which uses TcpStream::connect (calls getaddrinfo internally).
                        let pool_op = match addr {
                            ConnectAddr::Tcp { addr: host, port } => PoolOp::ConnectTcp {
                                addr: crate::io::sockaddr::format_host_port(host, *port),
                            },
                            ConnectAddr::Unix { path } => {
                                PoolOp::ConnectUnix { path: path.clone() }
                            }
                        };
                        network_pool.submit(id, pool_op)?;
                        None
                    }
                }
            }
            PlatformBackend::ThreadPool(_) => {
                let _ = buffer_pool;
                let pool_op = match addr {
                    ConnectAddr::Tcp { addr: host, port } => PoolOp::ConnectTcp {
                        addr: crate::io::sockaddr::format_host_port(host, *port),
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

    /// Submit a DNS resolution. Always dispatched to the thread pool.
    fn submit_resolve(&self, hostname: &str) -> Result<u64, String> {
        let mut inner = self.inner.borrow_mut();
        let id = inner.next_id;
        inner.next_id += 1;
        let buf_handle = inner.buffer_pool.alloc(0);
        inner.network_pool.submit(
            id,
            PoolOp::Resolve {
                hostname: hostname.to_string(),
            },
        )?;
        inner.pending.insert(
            id,
            PendingOp::Resolve {
                buffer_handle: buf_handle,
            },
        );
        Ok(id)
    }

    /// Submit a file open operation. Open creates a new port, so
    /// request.port is Value::NIL — we handle it before the port guard.
    fn submit_open(
        &self,
        path: &str,
        flags: i32,
        mode: u32,
        direction: Direction,
        encoding: Encoding,
        _timeout: Option<Duration>,
    ) -> Result<u64, String> {
        let mut inner = self.inner.borrow_mut();
        let id = inner.next_id;
        inner.next_id += 1;
        let buf_handle = inner.buffer_pool.alloc(0);

        let c_path = std::ffi::CString::new(path)
            .map_err(|_| format!("port/open: path contains null byte: {}", path))?;

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
                crate::io::uring::submit_uring_open(
                    ring,
                    id,
                    &c_path,
                    flags,
                    mode,
                    _timeout,
                    buffer_pool,
                    buf_handle,
                )?;
            }
            PlatformBackend::ThreadPool(_) => {
                let _ = buffer_pool;
                network_pool.submit(
                    id,
                    PoolOp::Open {
                        path: c_path,
                        flags,
                        mode,
                    },
                )?;
            }
        }

        pending.insert(
            id,
            PendingOp::Open {
                path: path.to_string(),
                direction,
                encoding,
                buffer_handle: buf_handle,
            },
        );
        Ok(id)
    }

    fn submit_spawn(&self, req: &SpawnRequest) -> Result<u64, String> {
        use crate::value::heap::TableKey;

        let mut inner = self.inner.borrow_mut();
        let id = inner.next_id;
        inner.next_id += 1;
        let buf_handle = inner.buffer_pool.alloc(0);

        let mut cmd = std::process::Command::new(&req.program);
        cmd.args(&req.args);
        if let Some(ref env_pairs) = req.env {
            cmd.env_clear();
            for (k, v) in env_pairs {
                cmd.env(k, v);
            }
        }
        if let Some(ref dir) = req.cwd {
            cmd.current_dir(dir);
        }
        cmd.stdin(stdio_to_std(req.stdin));
        cmd.stdout(stdio_to_std(req.stdout));
        cmd.stderr(stdio_to_std(req.stderr));

        let result = match cmd.spawn() {
            Ok(mut child) => {
                let pid = child.id();
                let stdin_val = child
                    .stdin
                    .take()
                    .map(|s| {
                        crate::io::backend::pipe_to_port(
                            s,
                            crate::port::Direction::Write,
                            crate::port::Encoding::Binary,
                            pid,
                            "stdin",
                        )
                    })
                    .unwrap_or(Value::NIL);
                let stdout_val = child
                    .stdout
                    .take()
                    .map(|s| {
                        crate::io::backend::pipe_to_port(
                            s,
                            crate::port::Direction::Read,
                            crate::port::Encoding::Binary,
                            pid,
                            "stdout",
                        )
                    })
                    .unwrap_or(Value::NIL);
                let stderr_val = child
                    .stderr
                    .take()
                    .map(|s| {
                        crate::io::backend::pipe_to_port(
                            s,
                            crate::port::Direction::Read,
                            crate::port::Encoding::Binary,
                            pid,
                            "stderr",
                        )
                    })
                    .unwrap_or(Value::NIL);

                let handle = ProcessHandle::new(pid, child);
                let handle_val = Value::external("process", handle);

                let mut fields = std::collections::BTreeMap::new();
                fields.insert(TableKey::Keyword("pid".into()), Value::int(pid as i64));
                fields.insert(TableKey::Keyword("stdin".into()), stdin_val);
                fields.insert(TableKey::Keyword("stdout".into()), stdout_val);
                fields.insert(TableKey::Keyword("stderr".into()), stderr_val);
                fields.insert(TableKey::Keyword("process".into()), handle_val);
                Ok(Value::struct_from(fields))
            }
            Err(e) => Err(error_val(
                "exec-error",
                format!("subprocess/exec: {}: {}", req.program, e),
            )),
        };

        inner.completions.push_back(Completion { id, result });

        // Spawn is an immediate completion — no CQE will arrive.
        // Release the placeholder buffer (was alloc(0), nothing stored).
        inner.buffer_pool.release(buf_handle);
        Ok(id)
    }

    fn submit_task(&self, task_fn: &TaskFn) -> Result<u64, String> {
        let closure = task_fn
            .take()
            .ok_or_else(|| "io/submit: task closure already consumed".to_string())?;
        let mut inner = self.inner.borrow_mut();
        let id = inner.next_id;
        inner.next_id += 1;
        let buf_handle = inner.buffer_pool.alloc(0);

        let AsyncBackendInner {
            ref mut platform,
            network_pool: ref mut _network_pool,
            ref mut pending,
            ..
        } = *inner;

        // Tasks always go to the thread pool (no io_uring equivalent).
        match platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(_) => {
                // Even on io_uring platforms, tasks run on the network pool
                // to avoid starving fd I/O ops on the main pool.
                _network_pool.submit(id, PoolOp::Task(closure))?;
            }
            PlatformBackend::ThreadPool(ref mut pool) => {
                pool.submit(id, PoolOp::Task(closure))?;
            }
        }

        pending.insert(
            id,
            PendingOp::Task {
                buffer_handle: buf_handle,
            },
        );
        Ok(id)
    }

    fn submit_process_wait(&self, handle_val: &Value) -> Result<u64, String> {
        let handle = handle_val
            .as_external::<ProcessHandle>()
            .ok_or_else(|| "io/submit: ProcessWait requires a process handle".to_string())?;

        // Fast path: already exited (cached). Push immediate completion, no pending entry.
        {
            let state = handle.inner.borrow();
            if let ProcessState::Exited(code) = &*state {
                let mut inner = self.inner.borrow_mut();
                let id = inner.next_id;
                inner.next_id += 1;
                inner.completions.push_back(Completion {
                    id,
                    result: Ok(Value::int(*code as i64)),
                });
                return Ok(id);
            }
        }

        let pid = handle.pid();
        let mut inner = self.inner.borrow_mut();
        let id = inner.next_id;
        inner.next_id += 1;
        let buf_handle = inner.buffer_pool.alloc(0);

        // Allocate siginfo_t for the kernel to fill on child exit.
        // Must live until the CQE arrives — stored in PendingOp.
        // SAFETY: zeroed() is valid for siginfo_t (all-zero is a valid initialized state).
        let siginfo_ptr = {
            let si: Box<libc::siginfo_t> = unsafe { Box::new(std::mem::zeroed()) };
            Box::into_raw(si)
        };

        let AsyncBackendInner {
            ref mut platform,
            ref mut pending,
            ..
        } = *inner;

        match platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ring) => {
                if let Err(e) =
                    crate::io::uring::submit_uring_process_wait(ring, id, pid, siginfo_ptr)
                {
                    // SAFETY: we own siginfo_ptr, just allocated above; reclaim it on error.
                    unsafe { drop(Box::from_raw(siginfo_ptr)) };
                    return Err(e);
                }
            }
            PlatformBackend::ThreadPool(ref mut pool) => {
                // No siginfo needed for thread pool path — reclaim the allocation.
                unsafe { drop(Box::from_raw(siginfo_ptr)) };
                pool.submit(id, PoolOp::ProcessWait { pid })?;
            }
        }

        // For the thread pool path, siginfo_ptr was already freed above.
        // Store null so the completion handler knows to use the raw result integer.
        let stored_siginfo = match platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(_) => siginfo_ptr,
            PlatformBackend::ThreadPool(_) => std::ptr::null_mut(),
        };

        pending.insert(
            id,
            PendingOp::ProcessWait {
                buffer_handle: buf_handle,
                handle_val: *handle_val,
                siginfo: stored_siginfo,
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

        // When stdin has pending ops and io_uring has nothing submitted,
        // block on the stdin receiver directly instead of io_uring (which
        // would block forever). This happens when the MCP server is a
        // subprocess reading from a pipe via StdinThread.
        if let Some(ref stdin_thread) = inner.stdin_thread {
            // Check if all pending ops are stdin ops. Stdin ops go through
            // StdinThread, not io_uring. If io_uring has nothing to wait on,
            // block on the stdin receiver directly.
            let all_pending_are_stdin = !inner.pending.is_empty()
                && inner.pending.values().all(|op| {
                    matches!(
                        op,
                        PendingOp::Port {
                            port_key: PortKey::Stdin,
                            ..
                        }
                    )
                });

            if all_pending_are_stdin {
                // All pending ops are stdin ops — block on stdin receiver.
                let timeout_dur = timeout.map(std::time::Duration::from_millis);
                let recv_result = match timeout_dur {
                    Some(dur) => stdin_thread.receiver().recv_timeout(dur).ok(),
                    None => stdin_thread.receiver().recv().ok(),
                };
                if let Some(sc) = recv_result {
                    if let Some(pending_op) = inner.pending.remove(&sc.id) {
                        inner.buffer_pool.release(pending_op.buffer_handle());
                        let c = match sc.result {
                            Ok(data) if data.is_empty() => Completion {
                                id: sc.id,
                                result: Ok(Value::NIL), // EOF
                            },
                            Ok(data) => Completion {
                                id: sc.id,
                                result: Ok(Value::string(
                                    String::from_utf8_lossy(&data)
                                        .trim_end_matches('\n')
                                        .trim_end_matches('\r'),
                                )),
                            },
                            Err(e) => Completion {
                                id: sc.id,
                                result: Err(error_val("io-error", e)),
                            },
                        };
                        inner.completions.push_back(c);
                    }
                }
                return Ok(inner.completions.drain(..).collect());
            }
        }

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
                    if network_pool.has_in_flight() {
                        // Network pool has in-flight ops (Resolve, hostname Connect
                        // fallback). Poll uring non-blocking, then wait on network
                        // pool with the caller's timeout so we don't miss completions
                        // from either source.
                        crate::io::uring::wait_uring(
                            ring,
                            Some(0), // poll only
                            pending,
                            buffer_pool,
                            fd_states,
                            completions,
                        )?;
                        let raw = network_pool.wait(Some(timeout.unwrap_or(100).min(100)))?;
                        for crate::io::threadpool::PoolCompletion {
                            id: cid,
                            result_code,
                            data,
                        } in raw
                        {
                            if let Some(mut pending_op) = pending.remove(&cid) {
                                if let PendingOp::Connect {
                                    ref mut connect_fd, ..
                                } = pending_op
                                {
                                    if result_code > 0 {
                                        *connect_fd = Some(result_code);
                                    }
                                }
                                let bh = pending_op.buffer_handle();
                                let c = completion::process_raw_completion(
                                    cid,
                                    result_code,
                                    data,
                                    &pending_op,
                                    fd_states,
                                    buffer_pool,
                                    bh,
                                );
                                completions.push_back(c);
                            }
                        }
                    } else {
                        crate::io::uring::wait_uring(
                            ring,
                            timeout,
                            pending,
                            buffer_pool,
                            fd_states,
                            completions,
                        )?;
                    }
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

impl crate::io::IoBackend for AsyncBackend {
    fn submit(&self, request: &IoRequest) -> Result<u64, String> {
        self.submit(request)
    }

    fn poll(&self) -> Vec<Completion> {
        self.poll()
    }

    fn wait(&self, timeout_ms: i64) -> Result<Vec<Completion>, String> {
        self.wait(timeout_ms)
    }

    fn cancel(&self, id: u64) -> Result<(), String> {
        self.cancel(id)
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
            | IoOp::Sleep { .. }
            | IoOp::Spawn(_)
            | IoOp::ProcessWait
            | IoOp::Open { .. }
            | IoOp::Seek { .. }
            | IoOp::Tell
            | IoOp::Task(_)
            | IoOp::Resolve { .. }
            | IoOp::Close => return Err("io/submit: unsupported operation on stdin".into()),
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

    /// Handle Seek and Tell as immediate completions.
    ///
    /// Called from AsyncBackend::submit after port_key is determined and before
    /// buffer allocation. Seek/Tell are synchronous (non-blocking lseek calls)
    /// and never go to io_uring or the thread pool.
    ///
    /// # Buffer invariant
    /// After Seek: the per-fd buffer is cleared and status reset to Open.
    /// After Tell: buffer is read-only; the formula is kernel_offset - buffer.len().
    fn handle_seek_tell(
        &mut self,
        id: u64,
        port: &Port,
        port_key: &PortKey,
        op: &IoOp,
    ) -> Result<u64, String> {
        if port.kind() != PortKind::File {
            let err_msg = match op {
                IoOp::Seek { .. } => {
                    format!("port/seek: expected file port, got {:?}", port.kind())
                }
                IoOp::Tell => format!("port/tell: expected file port, got {:?}", port.kind()),
                _ => unreachable!(),
            };
            self.completions.push_back(Completion {
                id,
                result: Err(error_val("type-error", err_msg)),
            });
            return Ok(id);
        }

        let result = match op {
            IoOp::Seek { offset, whence } => {
                // Discard buffered bytes — kernel offset and logical position diverge otherwise.
                if let Some(state) = self.fd_states.get_mut(port_key) {
                    state.buffer.clear();
                    state.status = crate::io::types::FdStatus::Open;
                }
                port.with_fd(|fd| {
                    let raw = fd.as_raw_fd();
                    let ret = unsafe { libc::lseek(raw, *offset, *whence) };
                    if ret < 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(Value::int(ret as i64))
                    }
                })
                .unwrap_or_else(|| {
                    Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "port/seek: fd unavailable",
                    ))
                })
            }
            IoOp::Tell => {
                let buffer_len: i64 = self
                    .fd_states
                    .get(port_key)
                    .map(|state| state.buffer.len() as i64)
                    .unwrap_or(0);
                port.with_fd(|fd| {
                    let raw = fd.as_raw_fd();
                    let ret = unsafe { libc::lseek(raw, 0, libc::SEEK_CUR) };
                    if ret < 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(Value::int(ret as i64 - buffer_len))
                    }
                })
                .unwrap_or_else(|| {
                    Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "port/tell: fd unavailable",
                    ))
                })
            }
            _ => unreachable!(),
        };

        self.completions.push_back(Completion {
            id,
            result: result.map_err(|e| error_val("io-error", e.to_string())),
        });
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

fn stdio_to_std(disp: StdioDisposition) -> std::process::Stdio {
    use std::process::Stdio;
    match disp {
        StdioDisposition::Pipe => Stdio::piped(),
        StdioDisposition::Inherit => Stdio::inherit(),
        StdioDisposition::Null => Stdio::null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::request::{IoOp, IoRequest};
    use crate::port::{Direction, Encoding, Port};
    use crate::value::heap::TableKey;
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

    /// Regression test: wait() must not return 0 completions when an accept
    /// SQE is in-flight and a connection arrives within the timeout window.
    ///
    /// Previously, submit_with_args() could return early (EINTR or spurious
    /// wakeup) and the discarded error caused wait() to return 0 completions
    /// even though the accept had not yet completed. The fix: loop wait() until
    /// at least one completion arrives or the deadline passes.
    #[test]
    fn test_accept_wait_does_not_return_zero_completions_spuriously() {
        use std::os::unix::io::FromRawFd;
        use std::sync::{Arc, Barrier};

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
            assert_eq!(
                libc::bind(
                    fd,
                    &addr as *const _ as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t
                ),
                0
            );
            assert_eq!(libc::listen(fd, 128), 0);
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

        // Use a barrier so the connect happens only after we're about to call wait().
        // This maximises the chance that wait() sees 0 completions on the first
        // drain and must block — the scenario where the spurious-return bug fires.
        let barrier = Arc::new(Barrier::new(2));
        let barrier2 = barrier.clone();
        let handle = std::thread::spawn(move || {
            barrier2.wait(); // released just before wait() is called
            std::net::TcpStream::connect(format!("127.0.0.1:{}", bound_port)).unwrap()
        });

        barrier.wait(); // release the connector thread
                        // wait() must return exactly 1 completion — the accept.
                        // If it returns 0, the bug is confirmed.
        let completions = backend.wait(5000).unwrap();
        assert_eq!(
            completions.len(),
            1,
            "wait() returned {} completions — expected 1 (spurious early return bug)",
            completions.len()
        );
        assert_eq!(completions[0].id, accept_id);
        assert!(completions[0].result.is_ok());
        handle.join().unwrap();
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

    fn open_rw_port(path: &str) -> Value {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .unwrap();
        let fd: std::os::unix::io::OwnedFd = file.into();
        Value::external(
            "port",
            Port::new_file(fd, Direction::ReadWrite, Encoding::Text, path.to_string()),
        )
    }

    #[test]
    fn test_async_seek_returns_immediate_completion() {
        let backend = AsyncBackend::new().unwrap();
        let path = write_temp_file("hello world");
        let port = open_rw_port(&path);

        let req = IoRequest {
            op: IoOp::Seek {
                offset: 6,
                whence: libc::SEEK_SET,
            },
            port,
            timeout: None,
        };
        let id = backend.submit(&req).unwrap();

        // Seek is immediate — no wait needed
        let completions = backend.poll();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        assert!(completions[0].result.is_ok());
        assert_eq!(completions[0].result.as_ref().unwrap().as_int(), Some(6));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_async_tell_returns_immediate_completion() {
        let backend = AsyncBackend::new().unwrap();
        let path = write_temp_file("hello");
        let port = open_rw_port(&path);

        let req = IoRequest {
            op: IoOp::Tell,
            port,
            timeout: None,
        };
        let id = backend.submit(&req).unwrap();

        let completions = backend.poll();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        assert!(completions[0].result.is_ok());
        assert_eq!(completions[0].result.as_ref().unwrap().as_int(), Some(0));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_async_seek_non_file_port_errors() {
        let backend = AsyncBackend::new().unwrap();
        let stdin_port = Value::external("port", Port::stdin());

        let req = IoRequest {
            op: IoOp::Seek {
                offset: 0,
                whence: libc::SEEK_SET,
            },
            port: stdin_port,
            timeout: None,
        };
        // stdin has PortKind::Stdin — seek must fail immediately
        let id = backend.submit(&req).unwrap();
        let completions = backend.poll();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        assert!(completions[0].result.is_err());
    }

    #[test]
    fn test_async_submit_spawn_echo() {
        use crate::io::request::{SpawnRequest, StdioDisposition};
        let backend = AsyncBackend::new().unwrap();
        let req = IoRequest {
            op: IoOp::Spawn(SpawnRequest {
                program: "/bin/echo".to_string(),
                args: vec!["hello-async".to_string()],
                env: None,
                cwd: None,
                stdin: StdioDisposition::Null,
                stdout: StdioDisposition::Pipe,
                stderr: StdioDisposition::Null,
            }),
            port: Value::NIL,
            timeout: None,
        };
        let id = backend.submit(&req).unwrap();
        let completions = backend.wait(-1).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        let val = completions[0].result.as_ref().expect("spawn failed");
        let fields = val.as_struct().expect("expected struct");
        assert!(
            fields
                .get(&TableKey::Keyword("pid".into()))
                .unwrap()
                .as_int()
                .unwrap()
                > 0
        );
    }

    /// Test IORING_OP_WAITID via async backend.
    /// Requires Linux kernel 6.7+. The test skips gracefully on older kernels
    /// by checking for -EINVAL completion.
    #[test]
    #[cfg(target_os = "linux")]
    fn test_async_submit_process_wait_uring() {
        use crate::io::request::{IoOp, IoRequest, ProcessHandle};

        let child = std::process::Command::new("/bin/true").spawn().unwrap();
        let pid = child.id();
        let handle = ProcessHandle::new(pid, child);
        let handle_val = Value::external("process", handle);

        let backend = AsyncBackend::new().unwrap();
        let req = IoRequest {
            op: IoOp::ProcessWait,
            port: handle_val,
            timeout: None,
        };
        let id = backend.submit(&req);

        match id {
            Err(e) if e.contains("thread-pool") => {
                // Thread-pool backend: ProcessWait not supported. Skip.
            }
            Err(e) => panic!("submit failed unexpectedly: {}", e),
            Ok(id) => {
                let completions = backend.wait(5000).unwrap();
                assert_eq!(completions.len(), 1);
                assert_eq!(completions[0].id, id);
                match &completions[0].result {
                    Err(e) => {
                        // -EINVAL means IORING_OP_WAITID not supported on this kernel. Skip.
                        let msg = format!("{:?}", e);
                        if msg.contains("22")
                            || msg.contains("EINVAL")
                            || msg.contains("waitid failed")
                        {
                            return; // kernel < 6.7
                        }
                        panic!("ProcessWait failed: {:?}", e);
                    }
                    Ok(val) => {
                        assert_eq!(val.as_int(), Some(0), "expected exit 0");
                    }
                }
            }
        }
    }

    // ── IoOp::Open integration tests ─────────────────────────────────────────

    #[test]
    fn test_async_open_regular_file_returns_port() {
        let path = format!("/tmp/elle-test-async-open-{}", std::process::id());
        std::fs::write(&path, "async open test").unwrap();

        let backend = AsyncBackend::new().unwrap();
        let req = IoRequest {
            op: IoOp::Open {
                path: path.clone(),
                flags: libc::O_RDONLY | libc::O_CLOEXEC,
                mode: 0o666,
                direction: crate::port::Direction::Read,
                encoding: crate::port::Encoding::Text,
            },
            port: Value::NIL,
            timeout: None,
        };
        let id = backend.submit(&req).unwrap();
        let completions = backend.wait(-1).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        assert!(
            completions[0].result.is_ok(),
            "open should succeed for existing file: {:?}",
            completions[0].result
        );
        // Result must be a port value
        let val = completions[0].result.as_ref().unwrap();
        assert_eq!(
            val.external_type_name(),
            Some("port"),
            "open result must be a port"
        );

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_async_open_nonexistent_path_errors() {
        let path = "/tmp/elle-test-async-open-nonexistent-dir/nofile";
        let backend = AsyncBackend::new().unwrap();
        let req = IoRequest {
            op: IoOp::Open {
                path: path.to_string(),
                flags: libc::O_RDONLY | libc::O_CLOEXEC,
                mode: 0o666,
                direction: crate::port::Direction::Read,
                encoding: crate::port::Encoding::Text,
            },
            port: Value::NIL,
            timeout: None,
        };
        let id = backend.submit(&req).unwrap();
        let completions = backend.wait(-1).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        assert!(
            completions[0].result.is_err(),
            "open must error for nonexistent path"
        );
    }

    #[test]
    fn test_async_open_with_timeout_succeeds_on_regular_file() {
        let path = format!("/tmp/elle-test-async-open-timeout-{}", std::process::id());
        std::fs::write(&path, "timeout test").unwrap();

        let backend = AsyncBackend::new().unwrap();
        let req = IoRequest {
            op: IoOp::Open {
                path: path.clone(),
                flags: libc::O_RDONLY | libc::O_CLOEXEC,
                mode: 0o666,
                direction: crate::port::Direction::Read,
                encoding: crate::port::Encoding::Text,
            },
            port: Value::NIL,
            timeout: Some(std::time::Duration::from_millis(5000)),
        };
        let id = backend.submit(&req).unwrap();
        let completions = backend.wait(-1).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, id);
        // Regular file opens instantly — should succeed before the 5s timeout.
        assert!(
            completions[0].result.is_ok(),
            "open with generous timeout must succeed for regular file: {:?}",
            completions[0].result
        );

        std::fs::remove_file(&path).ok();
    }
}
