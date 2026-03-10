//! Thread-pool backend and stdin thread for async I/O.

use std::os::unix::io::{IntoRawFd, RawFd};

pub(crate) struct ThreadPoolBackend {
    sender: crossbeam_channel::Sender<(u64, i32, Vec<u8>)>,
    receiver: crossbeam_channel::Receiver<(u64, i32, Vec<u8>)>,
    in_flight: usize,
}

/// Maximum concurrent thread-pool operations.
pub(super) const MAX_THREAD_POOL_OPS: usize = 64;

// Thread-pool op_kind values.
pub(super) const TP_OP_READ: u8 = 0;
pub(super) const TP_OP_WRITE: u8 = 1;
pub(super) const TP_OP_FLUSH: u8 = 2;
pub(super) const TP_OP_ACCEPT: u8 = 3;
pub(super) const TP_OP_CONNECT_TCP: u8 = 4;
pub(super) const TP_OP_CONNECT_UNIX: u8 = 5;
pub(super) const TP_OP_SEND_TO: u8 = 6;
pub(super) const TP_OP_RECV_FROM: u8 = 7;
pub(super) const TP_OP_SHUTDOWN: u8 = 8;

impl ThreadPoolBackend {
    pub(super) fn new() -> Self {
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
    pub(super) fn submit(
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
                TP_OP_READ => {
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
                TP_OP_WRITE => {
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
                TP_OP_FLUSH => {
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
                TP_OP_ACCEPT => {
                    // Accept: fd is listener fd
                    let mut addr_storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
                    let mut addr_len: libc::socklen_t =
                        std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
                    let new_fd = unsafe {
                        libc::accept(
                            fd,
                            &mut addr_storage as *mut _ as *mut libc::sockaddr,
                            &mut addr_len,
                        )
                    };
                    if new_fd < 0 {
                        (
                            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
                            Vec::new(),
                        )
                    } else {
                        unsafe {
                            libc::fcntl(new_fd, libc::F_SETFD, libc::FD_CLOEXEC);
                        }
                        // Encode addr_len + addr_storage as bytes for completion processing
                        let mut result_data = Vec::new();
                        result_data.extend_from_slice(&addr_len.to_le_bytes());
                        let addr_bytes = unsafe {
                            std::slice::from_raw_parts(
                                &addr_storage as *const _ as *const u8,
                                std::mem::size_of::<libc::sockaddr_storage>(),
                            )
                        };
                        result_data.extend_from_slice(addr_bytes);
                        (new_fd, result_data)
                    }
                }
                TP_OP_CONNECT_TCP => {
                    // Connect TCP: data = "addr:port" as UTF-8
                    let addr_str = String::from_utf8_lossy(&data).to_string();
                    match std::net::TcpStream::connect(&addr_str) {
                        Ok(stream) => {
                            let peer = stream
                                .peer_addr()
                                .map(|a| a.to_string())
                                .unwrap_or_else(|_| addr_str);
                            let new_fd = stream.into_raw_fd();
                            (new_fd, peer.into_bytes())
                        }
                        Err(e) => (
                            -(e.raw_os_error().unwrap_or(1)),
                            format!("{}", e).into_bytes(),
                        ),
                    }
                }
                TP_OP_CONNECT_UNIX => {
                    // Connect Unix: data = path as UTF-8
                    let path = String::from_utf8_lossy(&data).to_string();
                    let sock_fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
                    if sock_fd < 0 {
                        (
                            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
                            Vec::new(),
                        )
                    } else {
                        let mut sun: libc::sockaddr_un = unsafe { std::mem::zeroed() };
                        sun.sun_family = libc::AF_UNIX as libc::sa_family_t;
                        let (addr_len, ok) = if let Some(name) = path.strip_prefix('@') {
                            let max = sun.sun_path.len() - 1;
                            if name.len() > max {
                                (0 as libc::socklen_t, false)
                            } else {
                                sun.sun_path[0] = 0;
                                for (i, b) in name.bytes().enumerate() {
                                    sun.sun_path[i + 1] = b as libc::c_char;
                                }
                                let len = std::mem::size_of::<libc::sa_family_t>() + 1 + name.len();
                                (len as libc::socklen_t, true)
                            }
                        } else {
                            let max = sun.sun_path.len() - 1;
                            if path.len() > max {
                                (0 as libc::socklen_t, false)
                            } else {
                                for (i, b) in path.bytes().enumerate() {
                                    sun.sun_path[i] = b as libc::c_char;
                                }
                                let len = std::mem::size_of::<libc::sa_family_t>() + path.len() + 1;
                                (len as libc::socklen_t, true)
                            }
                        };
                        if !ok {
                            unsafe {
                                libc::close(sock_fd);
                            }
                            (-1, b"path too long".to_vec())
                        } else {
                            let ret = unsafe {
                                libc::connect(
                                    sock_fd,
                                    &sun as *const _ as *const libc::sockaddr,
                                    addr_len,
                                )
                            };
                            if ret < 0 {
                                let err = std::io::Error::last_os_error();
                                unsafe {
                                    libc::close(sock_fd);
                                }
                                (
                                    -(err.raw_os_error().unwrap_or(1)),
                                    format!("{}", err).into_bytes(),
                                )
                            } else {
                                unsafe {
                                    libc::fcntl(sock_fd, libc::F_SETFD, libc::FD_CLOEXEC);
                                }
                                (sock_fd, path.into_bytes())
                            }
                        }
                    }
                }
                TP_OP_SEND_TO => {
                    // SendTo: data = "addr:port\0payload"
                    let nul_pos = data.iter().position(|&b| b == 0).unwrap_or(data.len());
                    let addr_str = String::from_utf8_lossy(&data[..nul_pos]).to_string();
                    let payload = if nul_pos < data.len() {
                        &data[nul_pos + 1..]
                    } else {
                        &[]
                    };
                    match addr_str.parse::<std::net::SocketAddr>() {
                        Ok(dest) => {
                            let (sockaddr, sockaddr_len) = match dest {
                                std::net::SocketAddr::V4(v4) => {
                                    let mut sin: libc::sockaddr_in = unsafe { std::mem::zeroed() };
                                    sin.sin_family = libc::AF_INET as libc::sa_family_t;
                                    sin.sin_port = v4.port().to_be();
                                    sin.sin_addr.s_addr = u32::from(*v4.ip()).to_be();
                                    let ptr =
                                        &sin as *const libc::sockaddr_in as *const libc::sockaddr;
                                    let len =
                                        std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
                                    (ptr, len)
                                }
                                std::net::SocketAddr::V6(v6) => {
                                    let mut sin6: libc::sockaddr_in6 =
                                        unsafe { std::mem::zeroed() };
                                    sin6.sin6_family = libc::AF_INET6 as libc::sa_family_t;
                                    sin6.sin6_port = v6.port().to_be();
                                    sin6.sin6_addr.s6_addr = v6.ip().octets();
                                    let ptr =
                                        &sin6 as *const libc::sockaddr_in6 as *const libc::sockaddr;
                                    let len = std::mem::size_of::<libc::sockaddr_in6>()
                                        as libc::socklen_t;
                                    (ptr, len)
                                }
                            };
                            let ret = unsafe {
                                libc::sendto(
                                    fd,
                                    payload.as_ptr() as *const libc::c_void,
                                    payload.len(),
                                    0,
                                    sockaddr,
                                    sockaddr_len,
                                )
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
                        Err(e) => (-1, format!("bad address: {}", e).into_bytes()),
                    }
                }
                TP_OP_RECV_FROM => {
                    // RecvFrom: fd is socket fd, size is buffer size
                    let mut buf = vec![0u8; size];
                    let mut addr_storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
                    let mut addr_len: libc::socklen_t =
                        std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
                    let ret = unsafe {
                        libc::recvfrom(
                            fd,
                            buf.as_mut_ptr() as *mut libc::c_void,
                            buf.len(),
                            0,
                            &mut addr_storage as *mut _ as *mut libc::sockaddr,
                            &mut addr_len,
                        )
                    };
                    if ret < 0 {
                        (
                            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
                            Vec::new(),
                        )
                    } else {
                        buf.truncate(ret as usize);
                        // Encode: addr_len(4 bytes LE) + sockaddr_storage + data
                        let mut result_data = Vec::new();
                        result_data.extend_from_slice(&addr_len.to_le_bytes());
                        let addr_bytes = unsafe {
                            std::slice::from_raw_parts(
                                &addr_storage as *const _ as *const u8,
                                std::mem::size_of::<libc::sockaddr_storage>(),
                            )
                        };
                        result_data.extend_from_slice(addr_bytes);
                        result_data.extend_from_slice(&buf);
                        (ret as i32, result_data)
                    }
                }
                TP_OP_SHUTDOWN => {
                    // Shutdown: data[0] is `how` value
                    let how = if data.is_empty() { 0 } else { data[0] as i32 };
                    let ret = unsafe { libc::shutdown(fd, how) };
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
    pub(super) fn poll(&mut self) -> Vec<(u64, i32, Vec<u8>)> {
        let mut results = Vec::new();
        while let Ok(item) = self.receiver.try_recv() {
            self.in_flight -= 1;
            results.push(item);
        }
        results
    }

    /// Blocking wait for at least one completion.
    /// `timeout_ms`: None = wait forever, Some(0) = poll, Some(n) = wait up to n ms.
    pub(super) fn wait(
        &mut self,
        timeout_ms: Option<u64>,
    ) -> Result<Vec<(u64, i32, Vec<u8>)>, String> {
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

// --- StdinThread ---

/// Dedicated thread for blocking stdin reads.
///
/// stdin is blocking and cannot go through io_uring without blocking
/// a kernel worker thread. This thread serializes stdin reads through
/// a channel pair.
///
/// Drop order: request_tx drops first (closing channel), then completion_rx,
/// then handle (detaching thread). The thread exits on next recv() attempt.
/// No custom Drop impl needed.
pub(super) struct StdinThread {
    request_tx: crossbeam_channel::Sender<StdinRequest>,
    completion_rx: crossbeam_channel::Receiver<StdinCompletion>,
    /// Thread handle kept for Drop semantics: when dropped, the thread detaches.
    /// Not directly read, but essential for proper cleanup.
    #[allow(dead_code)]
    handle: std::thread::JoinHandle<()>,
}

pub(super) struct StdinRequest {
    id: u64,
    op_kind: StdinOpKind,
}

pub(super) enum StdinOpKind {
    ReadLine,
    Read { count: usize },
    ReadAll,
}

pub(super) struct StdinCompletion {
    pub(super) id: u64,
    pub(super) result: Result<Vec<u8>, String>,
}

impl StdinThread {
    pub(super) fn new() -> Self {
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

    pub(super) fn submit(&self, id: u64, op_kind: StdinOpKind) -> Result<(), String> {
        self.request_tx
            .send(StdinRequest { id, op_kind })
            .map_err(|_| "stdin thread channel disconnected".to_string())
    }

    pub(super) fn poll_completions(&self) -> Vec<StdinCompletion> {
        let mut results = Vec::new();
        while let Ok(c) = self.completion_rx.try_recv() {
            results.push(c);
        }
        results
    }
}
