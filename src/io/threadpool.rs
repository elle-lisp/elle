//! Thread-pool backend and stdin thread for async I/O.

use std::os::unix::io::{IntoRawFd, RawFd};

/// Typed thread-pool operation (replaces `op_kind: u8` + overloaded `data`/`size`/`fd`).
pub(super) enum PoolOp {
    Read {
        fd: RawFd,
        size: usize,
    },
    Write {
        fd: RawFd,
        data: Vec<u8>,
    },
    Flush {
        fd: RawFd,
    },
    Accept {
        fd: RawFd,
    },
    ConnectTcp {
        addr: String,
    },
    ConnectUnix {
        path: String,
    },
    SendTo {
        fd: RawFd,
        addr: String,
        port: u16,
        data: Vec<u8>,
    },
    RecvFrom {
        fd: RawFd,
        size: usize,
    },
    Shutdown {
        fd: RawFd,
        how: i32,
    },
    Sleep {
        nanos: u64,
    },
    ProcessWait {
        pid: u32,
    },
    /// Open a file asynchronously. Returns the fd (>= 0) on success, or -errno on failure.
    /// O_CLOEXEC is included in `flags` by the primitive — no post-hoc fcntl needed.
    Open {
        path: std::ffi::CString,
        flags: i32,
        mode: u32,
    },
    /// Run an arbitrary closure. Returns (result_code, data).
    Task(Box<dyn FnOnce() -> (i32, Vec<u8>) + Send>),
    /// Resolve a hostname via getaddrinfo(3). Returns IP addresses as
    /// newline-separated strings in `data`, result_code 0 on success.
    Resolve {
        hostname: String,
    },
    /// Read until a newline is found or EOF. Loops internally so the caller
    /// always receives data containing `\n` (or the final chunk at EOF).
    ReadLine {
        fd: RawFd,
    },
}

/// Typed thread-pool completion (replaces `(u64, i32, Vec<u8>)` tuples).
pub(super) struct PoolCompletion {
    pub(super) id: u64,
    pub(super) result_code: i32,
    pub(super) data: Vec<u8>,
}

pub(crate) struct ThreadPoolBackend {
    sender: crossbeam_channel::Sender<PoolCompletion>,
    receiver: crossbeam_channel::Receiver<PoolCompletion>,
    in_flight: usize,
}

/// Maximum concurrent thread-pool operations.
pub(super) const MAX_THREAD_POOL_OPS: usize = 64;

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
    pub(super) fn submit(&mut self, id: u64, op: PoolOp) -> Result<(), String> {
        if self.in_flight >= MAX_THREAD_POOL_OPS {
            return Err("async I/O: too many concurrent operations (max 64)".into());
        }
        let sender = self.sender.clone();
        self.in_flight += 1;
        std::thread::spawn(move || {
            let (result_code, data) = match op {
                PoolOp::Read { fd, size } => {
                    let mut buf = vec![0u8; size];
                    let mut total = 0usize;
                    loop {
                        let ret = unsafe {
                            libc::read(
                                fd,
                                buf[total..].as_mut_ptr() as *mut libc::c_void,
                                size - total,
                            )
                        };
                        if ret < 0 {
                            if total == 0 {
                                break (
                                    -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
                                    Vec::new(),
                                );
                            }
                            // Return whatever we accumulated before the error
                            break (total as i32, {
                                buf.truncate(total);
                                buf
                            });
                        }
                        if ret == 0 {
                            break (total as i32, {
                                buf.truncate(total);
                                buf
                            });
                        }
                        total += ret as usize;
                        if total >= size {
                            break (total as i32, buf);
                        }
                    }
                }
                PoolOp::ReadLine { fd } => {
                    let mut accumulated = Vec::new();
                    let mut chunk = vec![0u8; 4096];
                    loop {
                        let ret = unsafe {
                            libc::read(fd, chunk.as_mut_ptr() as *mut libc::c_void, chunk.len())
                        };
                        if ret < 0 {
                            if accumulated.is_empty() {
                                break (
                                    -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
                                    Vec::new(),
                                );
                            }
                            // Return whatever we accumulated before the error
                            break (accumulated.len() as i32, accumulated);
                        }
                        if ret == 0 {
                            // EOF — return whatever we have
                            break (accumulated.len() as i32, accumulated);
                        }
                        accumulated.extend_from_slice(&chunk[..ret as usize]);
                        if accumulated.contains(&b'\n') {
                            break (accumulated.len() as i32, accumulated);
                        }
                    }
                }
                PoolOp::Write { fd, data } => {
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
                PoolOp::Flush { fd } => {
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
                PoolOp::Accept { fd } => {
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
                PoolOp::ConnectTcp { addr } => match std::net::TcpStream::connect(&addr) {
                    Ok(stream) => {
                        let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or(addr);
                        let new_fd = stream.into_raw_fd();
                        (new_fd, peer.into_bytes())
                    }
                    Err(e) => (
                        -(e.raw_os_error().unwrap_or(1)),
                        format!("{}", e).into_bytes(),
                    ),
                },
                PoolOp::ConnectUnix { path } => {
                    let sock_fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
                    if sock_fd < 0 {
                        (
                            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
                            Vec::new(),
                        )
                    } else {
                        match crate::io::sockaddr::build_unix(&path) {
                            Err(msg) => {
                                unsafe { libc::close(sock_fd) };
                                (-1, msg.into_bytes())
                            }
                            Ok((sun, addr_len)) => {
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
                }
                PoolOp::SendTo {
                    fd,
                    addr,
                    port,
                    data,
                } => {
                    let addr_str = crate::io::sockaddr::format_host_port(&addr, port);
                    match addr_str.parse::<std::net::SocketAddr>() {
                        Ok(dest) => {
                            let (sa_bytes, sa_len) = crate::io::sockaddr::build_inet(&dest);
                            let ret = unsafe {
                                libc::sendto(
                                    fd,
                                    data.as_ptr() as *const libc::c_void,
                                    data.len(),
                                    0,
                                    sa_bytes.as_ptr() as *const libc::sockaddr,
                                    sa_len,
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
                PoolOp::RecvFrom { fd, size } => {
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
                PoolOp::Shutdown { fd, how } => {
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
                PoolOp::Sleep { nanos } => {
                    std::thread::sleep(std::time::Duration::from_nanos(nanos));
                    (0, Vec::new())
                }
                PoolOp::ProcessWait { pid } => {
                    let mut status: libc::c_int = 0;
                    let ret = unsafe { libc::waitpid(pid as libc::pid_t, &mut status, 0) };
                    if ret < 0 {
                        let code = -std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
                        (code, vec![])
                    } else {
                        let code = if libc::WIFEXITED(status) {
                            libc::WEXITSTATUS(status)
                        } else if libc::WIFSIGNALED(status) {
                            // killed by signal — return negative signal number by convention
                            -libc::WTERMSIG(status)
                        } else {
                            -1
                        };
                        (code, vec![])
                    }
                }
                PoolOp::Open { path, flags, mode } => {
                    let fd = unsafe {
                        libc::openat(
                            libc::AT_FDCWD,
                            path.as_ptr(),
                            flags,
                            libc::c_uint::from(mode as libc::mode_t),
                        )
                    };
                    if fd < 0 {
                        (
                            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
                            Vec::new(),
                        )
                    } else {
                        (fd, Vec::new())
                    }
                }
                PoolOp::Task(closure) => closure(),
                PoolOp::Resolve { hostname } => {
                    use std::net::ToSocketAddrs;
                    // getaddrinfo needs a "host:port" string; port 0 gets all addresses.
                    match (hostname.as_str(), 0u16).to_socket_addrs() {
                        Ok(addrs) => {
                            let ips: Vec<String> = addrs.map(|a| a.ip().to_string()).collect();
                            if ips.is_empty() {
                                (-1, b"getaddrinfo: no addresses found".to_vec())
                            } else {
                                (0, ips.join("\n").into_bytes())
                            }
                        }
                        Err(e) => (-1, format!("getaddrinfo: {}", e).into_bytes()),
                    }
                }
            };
            let _ = sender.send(PoolCompletion {
                id,
                result_code,
                data,
            });
        });
        Ok(())
    }

    /// Non-blocking poll for completions.
    pub(super) fn poll(&mut self) -> Vec<PoolCompletion> {
        let mut results = Vec::new();
        while let Ok(item) = self.receiver.try_recv() {
            self.in_flight -= 1;
            results.push(item);
        }
        results
    }

    /// Returns true if this pool has any in-flight operations.
    pub(super) fn has_in_flight(&self) -> bool {
        self.in_flight > 0
    }

    /// Expose the receiver for cross-pool select in async wait.
    pub(super) fn receiver(&self) -> &crossbeam_channel::Receiver<PoolCompletion> {
        &self.receiver
    }

    /// Record one completion received externally (via select).
    pub(super) fn record_completion(&mut self) {
        if self.in_flight > 0 {
            self.in_flight -= 1;
        }
    }

    /// Blocking wait for at least one completion.
    /// `timeout_ms`: None = wait forever, Some(0) = poll, Some(n) = wait up to n ms.
    pub(super) fn wait(&mut self, timeout_ms: Option<u64>) -> Result<Vec<PoolCompletion>, String> {
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

    /// Expose the receiver for cross-source select in async wait.
    pub(super) fn receiver(&self) -> &crossbeam_channel::Receiver<StdinCompletion> {
        &self.completion_rx
    }

    pub(super) fn poll_completions(&self) -> Vec<StdinCompletion> {
        let mut results = Vec::new();
        while let Ok(c) = self.completion_rx.try_recv() {
            results.push(c);
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_threadpool_process_wait_success() {
        let mut pool = ThreadPoolBackend::new();
        let mut child = std::process::Command::new("/bin/true").spawn().unwrap();
        let pid = child.id();
        pool.submit(1, PoolOp::ProcessWait { pid }).unwrap();
        let completions = pool.wait(Some(5000)).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, 1);
        assert_eq!(
            completions[0].result_code, 0,
            "expected exit code 0 from /bin/true"
        );
        // Reap from std::process::Child to avoid zombie
        let _ = child.wait();
    }

    #[test]
    fn test_threadpool_process_wait_failure() {
        let mut pool = ThreadPoolBackend::new();
        let mut child = std::process::Command::new("/bin/false").spawn().unwrap();
        let pid = child.id();
        pool.submit(2, PoolOp::ProcessWait { pid }).unwrap();
        let completions = pool.wait(Some(5000)).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, 2);
        assert_ne!(
            completions[0].result_code, 0,
            "expected non-zero exit code from /bin/false"
        );
        let _ = child.wait();
    }

    #[test]
    fn test_threadpool_open_existing_file_returns_valid_fd() {
        let path = "/tmp/elle-test-threadpool-open-success";
        std::fs::write(path, "test").unwrap();

        let mut pool = ThreadPoolBackend::new();
        let c_path = std::ffi::CString::new(path).unwrap();
        pool.submit(
            10,
            PoolOp::Open {
                path: c_path,
                flags: libc::O_RDONLY | libc::O_CLOEXEC,
                mode: 0o666,
            },
        )
        .unwrap();

        let completions = pool.wait(Some(5000)).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, 10);
        // result_code must be a valid fd (>= 0)
        let fd = completions[0].result_code;
        assert!(fd >= 0, "expected valid fd, got {}", fd);
        // Close the fd to avoid leaking it
        unsafe { libc::close(fd) };

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn test_threadpool_open_nonexistent_path_returns_negative_errno() {
        let path = "/tmp/elle-test-threadpool-open-nonexistent-dir/nofile";

        let mut pool = ThreadPoolBackend::new();
        let c_path = std::ffi::CString::new(path).unwrap();
        pool.submit(
            11,
            PoolOp::Open {
                path: c_path,
                flags: libc::O_RDONLY | libc::O_CLOEXEC,
                mode: 0o666,
            },
        )
        .unwrap();

        let completions = pool.wait(Some(5000)).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].id, 11);
        // result_code must be negative (errno)
        assert!(
            completions[0].result_code < 0,
            "expected negative errno for nonexistent path, got {}",
            completions[0].result_code
        );
    }
}
