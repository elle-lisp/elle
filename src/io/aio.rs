//! AsyncBackend — asynchronous I/O backend.
//!
//! Uses io_uring on Linux (feature-gated), thread-pool fallback elsewhere.
//! Wrapped as ExternalObject with type_name "io-backend" (same as SyncBackend).

use crate::io::pool::{BufferHandle, BufferPool};
use crate::io::request::{IoOp, IoRequest};
use crate::io::types::{FdState, FdStatus, PortKey};
use crate::port::{Encoding, Port, PortKind};
use crate::value::heap::TableKey;
use crate::value::{error_val, Value};

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::os::unix::io::{AsRawFd, RawFd};

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
struct PendingOp {
    op: IoOp,
    port_key: PortKey,
    port: Value,
    buffer_handle: BufferHandle,
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
}

// --- Platform backend dispatch ---

enum PlatformBackend {
    #[cfg(all(target_os = "linux", feature = "io-uring"))]
    Uring(Box<io_uring::IoUring>),
    ThreadPool(ThreadPoolBackend),
}

struct ThreadPoolBackend {
    sender: crossbeam_channel::Sender<(u64, i32, Vec<u8>)>,
    receiver: crossbeam_channel::Receiver<(u64, i32, Vec<u8>)>,
    in_flight: usize,
}

/// Maximum concurrent thread-pool operations.
const MAX_THREAD_POOL_OPS: usize = 64;

impl ThreadPoolBackend {
    fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        ThreadPoolBackend {
            sender,
            receiver,
            in_flight: 0,
        }
    }

    /// Submit a blocking I/O operation on a background thread.
    ///
    /// `fd` is the raw fd. `op_kind` is 0 for read, 1 for write.
    /// `data` is the write payload (empty for reads). `size` is the read buffer size.
    fn submit(
        &mut self,
        id: u64,
        fd: RawFd,
        op_kind: u8,
        data: Vec<u8>,
        size: usize,
    ) -> Result<(), String> {
        if self.in_flight >= MAX_THREAD_POOL_OPS {
            return Err("async I/O: too many concurrent operations (max 64)".into());
        }
        let sender = self.sender.clone();
        self.in_flight += 1;
        std::thread::spawn(move || {
            let (result_code, result_data) = match op_kind {
                0 => {
                    // Read
                    let mut buf = vec![0u8; size];
                    let ret =
                        unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
                    if ret < 0 {
                        (
                            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
                            Vec::new(),
                        )
                    } else {
                        buf.truncate(ret as usize);
                        (ret as i32, buf)
                    }
                }
                1 => {
                    // Write
                    let ret = unsafe {
                        libc::write(fd, data.as_ptr() as *const libc::c_void, data.len())
                    };
                    if ret < 0 {
                        (
                            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
                            Vec::new(),
                        )
                    } else {
                        (ret as i32, Vec::new())
                    }
                }
                2 => {
                    // Flush (fsync)
                    let ret = unsafe { libc::fsync(fd) };
                    if ret < 0 {
                        (
                            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
                            Vec::new(),
                        )
                    } else {
                        (0, Vec::new())
                    }
                }
                _ => (-1, Vec::new()),
            };
            let _ = sender.send((id, result_code, result_data));
        });
        Ok(())
    }

    /// Non-blocking poll for completions. Returns (id, result_code, data) tuples.
    fn poll(&mut self) -> Vec<(u64, i32, Vec<u8>)> {
        let mut results = Vec::new();
        while let Ok(item) = self.receiver.try_recv() {
            self.in_flight -= 1;
            results.push(item);
        }
        results
    }

    /// Blocking wait for at least one completion.
    /// `timeout_ms`: None = wait forever, Some(0) = poll, Some(n) = wait up to n ms.
    fn wait(&mut self, timeout_ms: Option<u64>) -> Result<Vec<(u64, i32, Vec<u8>)>, String> {
        let mut results = Vec::new();

        // First drain any already-available completions
        while let Ok(item) = self.receiver.try_recv() {
            self.in_flight -= 1;
            results.push(item);
        }
        if !results.is_empty() {
            return Ok(results);
        }

        // Nothing available — block for one
        match timeout_ms {
            Some(0) => Ok(results), // poll mode, already drained
            Some(ms) => {
                match self
                    .receiver
                    .recv_timeout(std::time::Duration::from_millis(ms))
                {
                    Ok(item) => {
                        self.in_flight -= 1;
                        results.push(item);
                        // Drain any more that arrived
                        while let Ok(item) = self.receiver.try_recv() {
                            self.in_flight -= 1;
                            results.push(item);
                        }
                        Ok(results)
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => Ok(results),
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        Err("async I/O: thread pool channel disconnected".into())
                    }
                }
            }
            None => {
                match self.receiver.recv() {
                    Ok(item) => {
                        self.in_flight -= 1;
                        results.push(item);
                        // Drain any more
                        while let Ok(item) = self.receiver.try_recv() {
                            self.in_flight -= 1;
                            results.push(item);
                        }
                        Ok(results)
                    }
                    Err(_) => Err("async I/O: thread pool channel disconnected".into()),
                }
            }
        }
    }
}

// --- StdinThread (placeholder for Chunk 5) ---

/// Dedicated thread for blocking stdin reads.
///
/// stdin is blocking and cannot go through io_uring without blocking
/// a kernel worker thread. This thread serializes stdin reads through
/// a channel pair.
///
/// Drop order: request_tx drops first (closing channel), then completion_rx,
/// then handle (detaching thread). The thread exits on next recv() attempt.
/// No custom Drop impl needed.
struct StdinThread {
    request_tx: crossbeam_channel::Sender<StdinRequest>,
    completion_rx: crossbeam_channel::Receiver<StdinCompletion>,
    /// Thread handle kept for Drop semantics: when dropped, the thread detaches.
    /// Not directly read, but essential for proper cleanup.
    #[allow(dead_code)]
    handle: std::thread::JoinHandle<()>,
}

struct StdinRequest {
    id: u64,
    op_kind: StdinOpKind,
}

enum StdinOpKind {
    ReadLine,
    Read { count: usize },
    ReadAll,
}

struct StdinCompletion {
    id: u64,
    result: Result<Vec<u8>, String>,
}

impl StdinThread {
    fn new() -> Self {
        let (request_tx, request_rx) = crossbeam_channel::unbounded::<StdinRequest>();
        let (completion_tx, completion_rx) = crossbeam_channel::unbounded::<StdinCompletion>();

        let handle = std::thread::Builder::new()
            .name("elle-stdin".into())
            .spawn(move || {
                use std::io::{BufRead, Read};
                while let Ok(req) = request_rx.recv() {
                    let result = match req.op_kind {
                        StdinOpKind::ReadLine => {
                            let mut line = String::new();
                            match std::io::stdin().lock().read_line(&mut line) {
                                Ok(0) => Ok(Vec::new()), // EOF
                                Ok(_) => {
                                    let trimmed =
                                        line.trim_end_matches('\n').trim_end_matches('\r');
                                    Ok(trimmed.as_bytes().to_vec())
                                }
                                Err(e) => Err(e.to_string()),
                            }
                        }
                        StdinOpKind::Read { count } => {
                            let mut buf = vec![0u8; count];
                            match std::io::stdin().lock().read(&mut buf) {
                                Ok(n) => {
                                    buf.truncate(n);
                                    Ok(buf)
                                }
                                Err(e) => Err(e.to_string()),
                            }
                        }
                        StdinOpKind::ReadAll => {
                            let mut buf = Vec::new();
                            match std::io::stdin().lock().read_to_end(&mut buf) {
                                Ok(_) => Ok(buf),
                                Err(e) => Err(e.to_string()),
                            }
                        }
                    };
                    let _ = completion_tx.send(StdinCompletion { id: req.id, result });
                }
            })
            .expect("failed to spawn stdin thread");

        StdinThread {
            request_tx,
            completion_rx,
            handle,
        }
    }

    fn submit(&self, id: u64, op_kind: StdinOpKind) -> Result<(), String> {
        self.request_tx
            .send(StdinRequest { id, op_kind })
            .map_err(|_| "stdin thread channel disconnected".to_string())
    }

    fn poll_completions(&self) -> Vec<StdinCompletion> {
        let mut results = Vec::new();
        while let Ok(c) = self.completion_rx.try_recv() {
            results.push(c);
        }
        results
    }
}

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

        let port_key = Self::port_key(port);

        // For stdin, route to stdin thread (see Chunk 5)
        if matches!(port_key, PortKey::Stdin) {
            return inner.submit_stdin(id, &request.op);
        }

        // Determine fd and operation parameters
        let fd = match &port_key {
            PortKey::Stdout => 1,
            PortKey::Stderr => 2,
            PortKey::Fd(raw) => *raw,
            PortKey::Stdin => unreachable!(), // handled above
        };

        let buf_handle = inner.buffer_pool.alloc(4096);

        // Determine op_kind and write data
        let (op_kind, write_data) = match &request.op {
            IoOp::ReadLine | IoOp::Read { .. } | IoOp::ReadAll | IoOp::RecvFrom { .. } => {
                (0u8, Vec::new())
            }
            IoOp::Write { data } | IoOp::SendTo { data, .. } => {
                let bytes = Self::extract_write_bytes(data);
                (1u8, bytes)
            }
            IoOp::Flush => (2u8, Vec::new()),
            // Network ops that don't map to simple read/write/flush
            IoOp::Accept | IoOp::Connect { .. } | IoOp::Shutdown { .. } => {
                return Err("async backend: network ops not yet implemented".into())
            }
        };

        let read_size = match &request.op {
            IoOp::Read { count } => *count,
            IoOp::ReadLine | IoOp::ReadAll => 4096,
            _ => 0,
        };

        // Submit to platform backend
        // Destructure to get independent borrows of each field.
        #[allow(unused_variables)]
        let AsyncBackendInner {
            ref mut platform,
            ref mut buffer_pool,
            ref mut pending,
            ..
        } = *inner;

        match platform {
            #[cfg(all(target_os = "linux", feature = "io-uring"))]
            PlatformBackend::Uring(ring) => {
                Self::submit_uring(
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
                    IoOp::RecvFrom { count } => IoOp::RecvFrom { count: *count },
                    IoOp::SendTo {
                        addr,
                        port_num,
                        data,
                    } => IoOp::SendTo {
                        addr: addr.clone(),
                        port_num: *port_num,
                        data: *data,
                    },
                    // These return early above, so unreachable here
                    IoOp::Accept | IoOp::Connect { .. } | IoOp::Shutdown { .. } => unreachable!(),
                },
                port_key,
                port: request.port,
                buffer_handle: buf_handle,
            },
        );

        Ok(id)
    }

    #[cfg(all(target_os = "linux", feature = "io-uring"))]
    #[allow(clippy::too_many_arguments)]
    fn submit_uring(
        ring: &mut io_uring::IoUring,
        id: u64,
        fd: RawFd,
        op_kind: u8,
        write_data: &[u8],
        read_size: usize,
        buffer_pool: &mut BufferPool,
        buf_handle: BufferHandle,
    ) -> Result<(), String> {
        use io_uring::opcode;
        use io_uring::types::Fd;

        let entry = match op_kind {
            0 => {
                // Read
                let buf = buffer_pool.get_mut(buf_handle);
                buf.resize(read_size, 0);
                opcode::Read::new(Fd(fd), buf.as_mut_ptr(), buf.len() as u32)
                    .build()
                    .user_data(id)
            }
            1 => {
                // Write — copy data into pool buffer
                let buf = buffer_pool.get_mut(buf_handle);
                buf.clear();
                buf.extend_from_slice(write_data);
                opcode::Write::new(Fd(fd), buf.as_ptr(), buf.len() as u32)
                    .build()
                    .user_data(id)
            }
            2 => {
                // Fsync
                opcode::Fsync::new(Fd(fd)).build().user_data(id)
            }
            _ => return Err("io/submit: unknown operation kind".into()),
        };

        unsafe {
            ring.submission()
                .push(&entry)
                .map_err(|_| "io/submit: io_uring submission queue full".to_string())?;
        }
        ring.submit()
            .map_err(|e| format!("io/submit: io_uring submit failed: {}", e))?;
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
                ref mut pending,
                ref mut buffer_pool,
                ref mut fd_states,
                ref mut completions,
                ..
            } = *inner;

            match platform {
                #[cfg(all(target_os = "linux", feature = "io-uring"))]
                PlatformBackend::Uring(ring) => {
                    Self::wait_uring(ring, timeout, pending, buffer_pool, fd_states, completions)?;
                }
                PlatformBackend::ThreadPool(pool) => {
                    let raw_completions = pool.wait(timeout)?;
                    for (id, result_code, data) in raw_completions {
                        if let Some(pending_op) = pending.remove(&id) {
                            let buf_handle = pending_op.buffer_handle;
                            // Release buffer first
                            buffer_pool.release(buf_handle);

                            // Process completion
                            let completion = if result_code < 0 {
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
                            completions.push_back(completion);
                        }
                    }
                }
            }
        }

        // Also drain stdin completions
        inner.drain_stdin_completions();

        Ok(inner.completions.drain(..).collect())
    }

    #[cfg(all(target_os = "linux", feature = "io-uring"))]
    fn wait_uring(
        ring: &mut io_uring::IoUring,
        timeout: Option<u64>,
        pending: &mut HashMap<u64, PendingOp>,
        buffer_pool: &mut BufferPool,
        fd_states: &mut HashMap<PortKey, FdState>,
        completions: &mut VecDeque<Completion>,
    ) -> Result<(), String> {
        // Wait for at least one CQE
        match timeout {
            Some(0) => {} // poll only
            Some(ms) => {
                let ts = io_uring::types::Timespec::new()
                    .sec(ms / 1000)
                    .nsec(((ms % 1000) * 1_000_000) as u32);
                let args = io_uring::types::SubmitArgs::new().timespec(&ts);
                let _ = ring.submitter().submit_with_args(1, &args);
            }
            None => {
                ring.submit_and_wait(1)
                    .map_err(|e| format!("io/wait: io_uring wait failed: {}", e))?;
            }
        }

        // Drain all available CQEs
        for cqe in ring.completion() {
            let id = cqe.user_data();
            let result_code = cqe.result();
            if let Some(pending_op) = pending.remove(&id) {
                let data = if result_code > 0 {
                    let buf = buffer_pool.get_mut(pending_op.buffer_handle);
                    buf[..result_code as usize].to_vec()
                } else {
                    Vec::new()
                };
                let completion = Self::process_raw_completion(
                    id,
                    result_code,
                    data,
                    &pending_op,
                    fd_states,
                    buffer_pool,
                    pending_op.buffer_handle,
                );
                completions.push_back(completion);
            }
        }
        Ok(())
    }

    /// Convert a raw (id, result_code, data) into a Completion with proper Value.
    fn process_raw_completion(
        id: u64,
        result_code: i32,
        data: Vec<u8>,
        pending: &PendingOp,
        fd_states: &mut HashMap<PortKey, FdState>,
        buffer_pool: &mut BufferPool,
        buf_handle: BufferHandle,
    ) -> Completion {
        // Release the buffer back to the pool
        buffer_pool.release(buf_handle);

        if result_code < 0 {
            // Error
            let errno = -result_code;
            let msg = format!("I/O error: errno {}", errno);
            let state = fd_states
                .entry(pending.port_key.clone())
                .or_insert_with(FdState::new);
            state.status = FdStatus::Error(msg.clone());
            Completion {
                id,
                result: Err(error_val("io-error", msg)),
            }
        } else if result_code == 0
            && matches!(
                pending.op,
                IoOp::ReadLine | IoOp::Read { .. } | IoOp::ReadAll
            )
        {
            // EOF for read operations
            let state = fd_states
                .entry(pending.port_key.clone())
                .or_insert_with(FdState::new);
            state.status = FdStatus::Eof;
            Completion {
                id,
                result: Ok(Value::NIL),
            }
        } else {
            // Success
            let value = match &pending.op {
                IoOp::ReadLine => {
                    let s = String::from_utf8_lossy(&data);
                    let trimmed = s.trim_end_matches('\n').trim_end_matches('\r');
                    Value::string(trimmed)
                }
                IoOp::Read { .. } | IoOp::ReadAll => {
                    // Check port encoding
                    if let Some(port) = pending.port.as_external::<Port>() {
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
                IoOp::Write { .. } | IoOp::SendTo { .. } => Value::int(result_code as i64),
                IoOp::Flush | IoOp::Shutdown { .. } => Value::NIL,
                IoOp::RecvFrom { .. } => Value::bytes(data),
                IoOp::Accept | IoOp::Connect { .. } => Value::NIL,
            };
            Completion {
                id,
                result: Ok(value),
            }
        }
    }

    fn extract_write_bytes(data: &Value) -> Vec<u8> {
        if let Some(s) = data.with_string(|s| s.as_bytes().to_vec()) {
            s
        } else if let Some(b) = data.as_bytes() {
            b.to_vec()
        } else if let Some(b) = data.as_blob() {
            b.borrow().clone()
        } else if let Some(b) = data.as_buffer() {
            b.borrow().clone()
        } else {
            format!("{}", data).into_bytes()
        }
    }

    fn port_key(port: &Port) -> PortKey {
        match port.kind() {
            PortKind::Stdin => PortKey::Stdin,
            PortKind::Stdout => PortKey::Stdout,
            PortKind::Stderr => PortKey::Stderr,
            PortKind::File
            | PortKind::TcpListener
            | PortKind::TcpStream
            | PortKind::UdpSocket
            | PortKind::UnixListener
            | PortKind::UnixStream => match port.with_fd(|fd| fd.as_raw_fd()) {
                Some(raw) => PortKey::Fd(raw),
                None => PortKey::Fd(-1),
            },
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
                    let id = cqe.user_data();
                    let result_code = cqe.result();
                    if let Some(pending) = self.pending.remove(&id) {
                        let data = if result_code > 0 {
                            let buf = self.buffer_pool.get_mut(pending.buffer_handle);
                            buf[..result_code as usize].to_vec()
                        } else {
                            Vec::new()
                        };
                        let completion = AsyncBackend::process_raw_completion(
                            id,
                            result_code,
                            data,
                            &pending,
                            &mut self.fd_states,
                            &mut self.buffer_pool,
                            pending.buffer_handle,
                        );
                        self.completions.push_back(completion);
                    }
                }
            }
            PlatformBackend::ThreadPool(pool) => {
                let raw = pool.poll();
                for (id, result_code, data) in raw {
                    if let Some(pending) = self.pending.remove(&id) {
                        let completion = AsyncBackend::process_raw_completion(
                            id,
                            result_code,
                            data,
                            &pending,
                            &mut self.fd_states,
                            &mut self.buffer_pool,
                            pending.buffer_handle,
                        );
                        self.completions.push_back(completion);
                    }
                }
            }
        }
    }

    /// Submit a stdin operation (placeholder — implemented in Chunk 5).
    fn submit_stdin(&mut self, id: u64, op: &IoOp) -> Result<u64, String> {
        // TODO(chunk5): stdin thread
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
            },
        );
        Ok(id)
    }

    /// Drain stdin completions (placeholder — implemented in Chunk 5).
    fn drain_stdin_completions(&mut self) {
        let completions_to_add: Vec<Completion> = if let Some(ref stdin_thread) = self.stdin_thread
        {
            stdin_thread
                .poll_completions()
                .into_iter()
                .filter_map(|sc| {
                    if let Some(pending) = self.pending.remove(&sc.id) {
                        self.buffer_pool.release(pending.buffer_handle);
                        let completion = match sc.result {
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
                        Some(completion)
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
