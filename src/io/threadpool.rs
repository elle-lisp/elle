//! Thread-pool backend and stdin thread for async I/O.

use crate::io::grapheme_count_in_valid_prefix;
use crate::io::request::SocketOptions;
use std::os::unix::io::{IntoRawFd, RawFd};

/// Typed thread-pool operation (replaces `op_kind: u8` + overloaded `data`/`size`/`fd`).
pub(super) enum PoolOp {
    Read {
        fd: RawFd,
        size: usize,
    },
    /// Read exactly `size` units, looping until full or EOF/error.
    /// Units are bytes when `graphemes` is false and grapheme clusters
    /// when true — Elle strings are grapheme-counted, so a text-port
    /// `port/read-exact 50` must yield a string of `(length 50)`
    /// regardless of how many kernel bytes that took.  The worker
    /// keeps calling `read(2)` until the requested count is met,
    /// the peer closes, or an error fires.  On EOF before `size`,
    /// the completion path treats the partial result as nil.
    ReadExact {
        fd: RawFd,
        size: usize,
        graphemes: bool,
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
        options: SocketOptions,
    },
    ConnectUnix {
        path: String,
        options: SocketOptions,
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
    /// Read until EOF. Loops internally, accumulating all data.
    ReadAll {
        fd: RawFd,
    },
    /// Blocking read on an inotify/kqueue fd for filesystem watch events.
    WatchRead {
        fd: RawFd,
    },
    /// Blocking read on a signalfd (Linux) for POSIX signal deliveries.
    /// On macOS the corresponding op is `KqSigRead`.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    SigfdRead {
        fd: RawFd,
    },
    /// Blocking kevent() on a kqueue fd registered with EVFILT_SIGNAL (macOS).
    /// On Linux the corresponding op is `SigfdRead`.
    ///
    /// `signals` is the set the watcher is interested in. The worker
    /// unblocks them on its own thread before calling kevent() because
    /// kqueue's `EVFILT_SIGNAL` fires from the in-kernel delivery path
    /// — when every thread in the process blocks the signal the kernel
    /// parks it on the process pending list and the knote never
    /// activates (no thread is selected for delivery, so the kqueue
    /// hook in psignal_internal is never reached). `SignalReceiver::new`
    /// installs a process-wide no-op sigaction handler so the signal
    /// delivered to this thread does no harm.
    #[cfg_attr(any(target_os = "linux", target_os = "android"), allow(dead_code))]
    KqSigRead {
        fd: RawFd,
        signals: Vec<libc::c_int>,
    },
    /// Poll a raw fd for readiness via libc::poll(). Returns revents mask.
    PollFd {
        fd: RawFd,
        events: u32,
        timeout_ms: i32,
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

/// A short `(kind, fd)` description of a `PoolOp` for `io_trace`. `fd` is
/// `-1` for ops that carry no fd (connect-by-name, sleep, resolve, …).
/// Diagnostics only: a `tp-submit` line with no matching `tp-complete`
/// names the exact syscall a worker is wedged in — the signal that pins a
/// macOS threadpool hang (e.g. an `accept()` that a listening-socket
/// `shutdown()` failed to wake). Exhaustive on purpose so a new `PoolOp`
/// variant forces a decision here.
fn pool_op_desc(op: &PoolOp) -> (&'static str, RawFd) {
    match op {
        PoolOp::Read { fd, .. } => ("read", *fd),
        PoolOp::ReadExact { fd, .. } => ("read-exact", *fd),
        PoolOp::Write { fd, .. } => ("write", *fd),
        PoolOp::Flush { fd } => ("flush", *fd),
        PoolOp::Accept { fd } => ("accept", *fd),
        PoolOp::ConnectTcp { .. } => ("connect-tcp", -1),
        PoolOp::ConnectUnix { .. } => ("connect-unix", -1),
        PoolOp::SendTo { fd, .. } => ("sendto", *fd),
        PoolOp::RecvFrom { fd, .. } => ("recvfrom", *fd),
        PoolOp::Shutdown { fd, .. } => ("shutdown", *fd),
        PoolOp::Sleep { .. } => ("sleep", -1),
        PoolOp::ProcessWait { .. } => ("process-wait", -1),
        PoolOp::Open { .. } => ("open", -1),
        PoolOp::Task(_) => ("task", -1),
        PoolOp::Resolve { .. } => ("resolve", -1),
        PoolOp::ReadLine { fd } => ("read-line", *fd),
        PoolOp::ReadAll { fd } => ("read-all", *fd),
        PoolOp::WatchRead { fd } => ("watch-read", *fd),
        PoolOp::SigfdRead { fd } => ("sigfd-read", *fd),
        PoolOp::KqSigRead { fd, .. } => ("kq-sig-read", *fd),
        PoolOp::PollFd { fd, .. } => ("poll-fd", *fd),
    }
}

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
        let (op_kind, op_fd) = pool_op_desc(&op);
        let sender = self.sender.clone();
        self.in_flight += 1;
        crate::io::io_trace(format_args!(
            "tp-submit   id={id} op={op_kind} fd={op_fd} in_flight={}",
            self.in_flight
        ));
        std::thread::spawn(move || {
            // Block all signals on this worker so the kernel never selects
            // it as the delivery target for a watched POSIX signal.
            // See src/io/sigfd.rs and docs/posix-signals.md.
            crate::io::sigfd::mask_all_signals_on_this_thread();
            let (result_code, data) = match op {
                PoolOp::Read { fd, size } => {
                    let mut buf = vec![0u8; size];
                    let ret =
                        unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, size) };
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
                PoolOp::ReadExact {
                    fd,
                    size,
                    graphemes,
                } => {
                    // Buffer grows as we go — graphemes mode can't preallocate
                    // because we don't know the byte count in advance.  In
                    // bytes mode we still grow into a `size`-capacity Vec so
                    // the loop's tail-read writes into one buffer.
                    let mut buf: Vec<u8> = if graphemes {
                        Vec::with_capacity(size)
                    } else {
                        vec![0u8; size]
                    };
                    let mut total = 0usize;
                    // chunk_size is how many bytes we ask the kernel for on
                    // each iteration.  Bytes mode knows exactly (size - total);
                    // graphemes mode estimates one byte per missing grapheme
                    // (ASCII best case) and loops on undershoot.
                    loop {
                        let want = if graphemes {
                            // Re-evaluate progress every iteration.
                            let g = grapheme_count_in_valid_prefix(&buf[..total]);
                            if g >= size {
                                break (total as i32, buf[..total].to_vec());
                            }
                            (size - g).max(1)
                        } else {
                            if total >= size {
                                break (total as i32, buf);
                            }
                            size - total
                        };
                        // Make room for the next read if we're in graphemes
                        // mode (bytes mode preallocated).
                        if graphemes && buf.len() < total + want {
                            buf.resize(total + want, 0);
                        }
                        let ret = unsafe {
                            libc::read(fd, buf[total..].as_mut_ptr() as *mut libc::c_void, want)
                        };
                        if ret < 0 {
                            if total == 0 {
                                break (
                                    -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
                                    Vec::new(),
                                );
                            }
                            // Partial read then error: surface what we got
                            // so the completion path treats it as short-then-EOF.
                            break (total as i32, buf[..total].to_vec());
                        }
                        if ret == 0 {
                            // EOF before full count.  Return short; the
                            // completion handler maps short-on-ReadExact to nil.
                            break (total as i32, buf[..total].to_vec());
                        }
                        total += ret as usize;
                    }
                }
                PoolOp::ReadLine { fd } | PoolOp::ReadAll { fd } => {
                    let until_newline = matches!(op, PoolOp::ReadLine { .. });
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
                        if until_newline && accumulated.contains(&b'\n') {
                            break (accumulated.len() as i32, accumulated);
                        }
                    }
                }
                PoolOp::Write { fd, data } => {
                    let mut total = 0usize;
                    loop {
                        let ret = unsafe {
                            libc::write(
                                fd,
                                data[total..].as_ptr() as *const libc::c_void,
                                data.len() - total,
                            )
                        };
                        if ret < 0 {
                            if total == 0 {
                                break (
                                    -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
                                    Vec::new(),
                                );
                            }
                            break (total as i32, Vec::new());
                        }
                        total += ret as usize;
                        if total >= data.len() {
                            break (total as i32, Vec::new());
                        }
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
                PoolOp::ConnectTcp { addr, options } => match std::net::TcpStream::connect(&addr) {
                    Ok(stream) => {
                        let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or(addr);
                        let new_fd = stream.into_raw_fd();
                        crate::io::request::apply_socket_options(new_fd, &options);
                        (new_fd, peer.into_bytes())
                    }
                    Err(e) => (
                        -(e.raw_os_error().unwrap_or(1)),
                        format!("{}", e).into_bytes(),
                    ),
                },
                PoolOp::ConnectUnix { path, options } => {
                    let sock_fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
                    if sock_fd < 0 {
                        (
                            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
                            Vec::new(),
                        )
                    } else {
                        crate::io::request::apply_socket_options(sock_fd, &options);
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
                        let exit_code: i32 = if libc::WIFEXITED(status) {
                            libc::WEXITSTATUS(status)
                        } else if libc::WIFSIGNALED(status) {
                            -libc::WTERMSIG(status)
                        } else {
                            -1
                        };
                        // Encode exit code in data so result_code=0 (success)
                        // avoids collision with negative errno in completion handler.
                        (0, exit_code.to_le_bytes().to_vec())
                    }
                }
                PoolOp::Open { path, flags, mode } => {
                    let fd = unsafe {
                        libc::openat(libc::AT_FDCWD, path.as_ptr(), flags, mode as libc::c_uint)
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
                PoolOp::WatchRead { fd } => watch_read_blocking(fd),
                #[cfg(any(target_os = "linux", target_os = "android"))]
                PoolOp::SigfdRead { fd } => sigfd_read_blocking(fd),
                #[cfg(not(any(target_os = "linux", target_os = "android")))]
                PoolOp::SigfdRead { .. } => (
                    -libc::ENOTSUP,
                    b"sig-next: signalfd not supported on this platform".to_vec(),
                ),
                #[cfg(target_os = "macos")]
                PoolOp::KqSigRead { fd, signals } => kq_sig_read_blocking(fd, &signals),
                #[cfg(not(target_os = "macos"))]
                PoolOp::KqSigRead { .. } => (
                    -libc::ENOTSUP,
                    b"sig-next: kqueue signal mode not supported on this platform".to_vec(),
                ),
                PoolOp::PollFd {
                    fd,
                    events,
                    timeout_ms,
                } => {
                    let mut pfd = libc::pollfd {
                        fd,
                        events: events as i16,
                        revents: 0,
                    };
                    let ret = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
                    if ret < 0 {
                        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
                        (-errno, Vec::new())
                    } else {
                        (pfd.revents as i32, Vec::new())
                    }
                }
            };
            crate::io::io_trace(format_args!(
                "tp-complete id={id} op={op_kind} fd={op_fd} rc={result_code}"
            ));
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
    #[allow(dead_code)]
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
    /// Write end of the cancellation self-pipe. Writing any byte here
    /// wakes the stdin thread out of `libc::poll` so it can either
    /// (a) acknowledge a shutdown and exit, or (b) treat an in-flight
    /// read as cancelled. Owned by us; closed in `Drop`.
    shutdown_write_fd: RawFd,
    /// Thread handle kept for join in tests and for `is_finished`
    /// observation. In production, the runtime calls `shutdown()` and
    /// then drops the thread; the thread exits within a few syscall
    /// hops of the shutdown write.
    handle: Option<std::thread::JoinHandle<()>>,
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

/// Sentinel string used in the cancelled completion's error message.
/// `(port/close *stdin*)` translates this into an `:io-error` whose
/// `:message` field is exactly `"stdin closed"`, matching the contract
/// documented in `docs/io.md`. Searched for by the threadpool tests.
const STDIN_CLOSED_MSG: &str = "stdin closed";

impl StdinThread {
    pub(super) fn new() -> Self {
        let (request_tx, request_rx) = crossbeam_channel::unbounded::<StdinRequest>();
        let (completion_tx, completion_rx) = crossbeam_channel::unbounded::<StdinCompletion>();

        // Self-pipe for cancellation. The thread polls the read end
        // alongside fd 0; writing any byte here wakes the poll(2).
        // We set the read end to O_NONBLOCK so the thread's drain
        // (after a shutdown wakeup) never blocks.
        let mut pipe_fds: [libc::c_int; 2] = [0; 2];
        let pipe_ret = unsafe { libc::pipe(pipe_fds.as_mut_ptr()) };
        if pipe_ret != 0 {
            panic!(
                "StdinThread: pipe(2) failed: {}",
                std::io::Error::last_os_error()
            );
        }
        let shutdown_read_fd = pipe_fds[0];
        let shutdown_write_fd = pipe_fds[1];
        unsafe {
            libc::fcntl(shutdown_read_fd, libc::F_SETFL, libc::O_NONBLOCK);
            libc::fcntl(shutdown_read_fd, libc::F_SETFD, libc::FD_CLOEXEC);
            libc::fcntl(shutdown_write_fd, libc::F_SETFD, libc::FD_CLOEXEC);
        }

        let handle = std::thread::Builder::new()
            .name("elle-stdin".into())
            .spawn(move || {
                crate::io::sigfd::mask_all_signals_on_this_thread();
                stdin_thread_loop(request_rx, completion_tx, shutdown_read_fd);
                unsafe { libc::close(shutdown_read_fd) };
            })
            .expect("failed to spawn stdin thread");

        StdinThread {
            request_tx,
            completion_rx,
            shutdown_write_fd,
            handle: Some(handle),
        }
    }

    pub(super) fn submit(&self, id: u64, op_kind: StdinOpKind) -> Result<(), String> {
        self.request_tx
            .send(StdinRequest { id, op_kind })
            .map_err(|_| "stdin thread channel disconnected".to_string())
    }

    /// Signal the stdin thread to shut down. The thread either:
    ///   - if currently inside `poll(2)` waiting for input on fd 0,
    ///     observes the shutdown pipe revents and sends a `stdin
    ///     closed` error completion for the in-flight request before
    ///     exiting;
    ///   - if currently waiting in `request_rx.recv_timeout`, picks
    ///     the shutdown up on its next 100 ms tick and exits.
    ///
    /// Idempotent: subsequent calls write extra bytes into the pipe
    /// which the thread either drains on exit or never reads (already
    /// gone). The write is bounded to 1 byte so it cannot ever
    /// block on a full kernel pipe buffer.
    pub(super) fn shutdown(&self) {
        let byte: u8 = 1;
        unsafe {
            libc::write(
                self.shutdown_write_fd,
                &byte as *const u8 as *const libc::c_void,
                1,
            );
        }
    }

    /// True once the worker thread has exited. Used by tests to assert
    /// `shutdown()` actually wound the thread down; callers in the
    /// runtime don't need this (the drop path waits for them).
    #[allow(dead_code)]
    pub(super) fn is_finished(&self) -> bool {
        self.handle.as_ref().is_none_or(|h| h.is_finished())
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

impl Drop for StdinThread {
    fn drop(&mut self) {
        // Signal shutdown so the worker exits promptly. Closing the
        // write end signals EOF on the pipe — the thread's poll picks
        // it up too — but `shutdown()` writes a byte first to wake
        // any current poll. Either is sufficient; both is robust.
        self.shutdown();
        unsafe { libc::close(self.shutdown_write_fd) };
        if let Some(h) = self.handle.take() {
            // Best-effort join. The thread is bounded by the next poll
            // tick (~100 ms) plus the time to send any pending
            // cancellation completion. In practice this returns
            // quickly; we tolerate a brief blip on Drop rather than
            // detaching and leaking a thread.
            let _ = h.join();
        }
    }
}

/// Main worker loop. Multiplexes:
///   - `request_rx`: incoming `(id, op_kind)` requests.
///   - `shutdown_read_fd`: byte arrival means caller asked us to die.
///   - fd 0: actual stdin input for the in-flight request.
///
/// We don't have a portable way to `select` simultaneously on a
/// crossbeam channel and a raw fd, so the loop alternates: idle ticks
/// poll the shutdown fd alongside a short `recv_timeout` on the
/// channel; the active state (mid-read) uses `poll(2)` on fd 0 plus
/// the shutdown fd.
fn stdin_thread_loop(
    request_rx: crossbeam_channel::Receiver<StdinRequest>,
    completion_tx: crossbeam_channel::Sender<StdinCompletion>,
    shutdown_read_fd: RawFd,
) {
    use std::time::Duration;
    let mut leftover: Vec<u8> = Vec::new();
    loop {
        if shutdown_signalled(shutdown_read_fd) {
            return;
        }
        let req = match request_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(r) => r,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
        };
        let result = match req.op_kind {
            StdinOpKind::ReadLine => read_line_with_cancel(shutdown_read_fd, &mut leftover),
            StdinOpKind::Read { count } => {
                read_n_with_cancel(shutdown_read_fd, count, &mut leftover)
            }
            StdinOpKind::ReadAll => read_all_with_cancel(shutdown_read_fd, &mut leftover),
        };
        let was_cancelled = matches!(&result, Err(s) if s == STDIN_CLOSED_MSG);
        let _ = completion_tx.send(StdinCompletion { id: req.id, result });
        if was_cancelled {
            // Drain any further queued requests as cancelled so their
            // submitters see a completion rather than hanging.
            while let Ok(r) = request_rx.try_recv() {
                let _ = completion_tx.send(StdinCompletion {
                    id: r.id,
                    result: Err(STDIN_CLOSED_MSG.to_string()),
                });
            }
            return;
        }
    }
}

/// Non-blocking peek at the shutdown pipe. Returns true if any byte
/// is present (we don't bother to drain — `read_*_with_cancel` will
/// see the revents in its next poll).
fn shutdown_signalled(shutdown_read_fd: RawFd) -> bool {
    let mut pfd = libc::pollfd {
        fd: shutdown_read_fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let ret = unsafe { libc::poll(&mut pfd, 1, 0) };
    ret > 0 && pfd.revents != 0
}

/// Poll fd 0 and `shutdown_read_fd` simultaneously. Returns `Ok(true)`
/// when stdin has input available, `Ok(false)` when shutdown was
/// signalled, `Err` on real syscall errors. The shutdown branch wins
/// any race so close-then-arrive-input doesn't leak data into a
/// completion the caller will never read.
fn poll_stdin_or_shutdown(shutdown_read_fd: RawFd) -> Result<bool, String> {
    loop {
        let mut pfds = [
            libc::pollfd {
                fd: 0,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: shutdown_read_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let ret = unsafe { libc::poll(pfds.as_mut_ptr(), 2, -1) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(format!("poll: {}", err));
        }
        // Shutdown wins races against pending stdin input.
        if pfds[1].revents != 0 {
            return Ok(false);
        }
        if pfds[0].revents != 0 {
            return Ok(true);
        }
        // No revents but poll > 0 should not happen; loop defensively.
    }
}

/// One iteration of "wait + read" against fd 0 with shutdown
/// observation. Returns:
///   - `Ok(Some(bytes))` — fresh bytes read.
///   - `Ok(None)` — EOF.
///   - `Err(STDIN_CLOSED_MSG)` — shutdown was signalled.
///   - `Err(other)` — real I/O error.
fn read_chunk_with_cancel(shutdown_read_fd: RawFd, max: usize) -> Result<Option<Vec<u8>>, String> {
    if !poll_stdin_or_shutdown(shutdown_read_fd)? {
        return Err(STDIN_CLOSED_MSG.to_string());
    }
    let mut buf = vec![0u8; max];
    loop {
        let ret = unsafe { libc::read(0, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            match err.raw_os_error() {
                Some(libc::EINTR) | Some(libc::EAGAIN) => {
                    // Re-poll: input may have been consumed by another
                    // reader, or the signal was harmless.
                    if !poll_stdin_or_shutdown(shutdown_read_fd)? {
                        return Err(STDIN_CLOSED_MSG.to_string());
                    }
                    continue;
                }
                _ => return Err(format!("read: {}", err)),
            }
        }
        if ret == 0 {
            return Ok(None);
        }
        buf.truncate(ret as usize);
        return Ok(Some(buf));
    }
}

fn read_line_with_cancel(
    shutdown_read_fd: RawFd,
    leftover: &mut Vec<u8>,
) -> Result<Vec<u8>, String> {
    // If a previous read consumed past a newline, the bytes after the
    // newline went into `leftover`. Serve from there first.
    if let Some(nl) = leftover.iter().position(|&b| b == b'\n') {
        let mut line: Vec<u8> = leftover.drain(..=nl).collect();
        line.pop(); // drop \n
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        return Ok(line);
    }
    // Read chunks until newline or EOF.
    let mut accum = std::mem::take(leftover);
    loop {
        match read_chunk_with_cancel(shutdown_read_fd, 4096)? {
            Some(bytes) => {
                accum.extend_from_slice(&bytes);
                if let Some(nl) = accum.iter().position(|&b| b == b'\n') {
                    let mut line: Vec<u8> = accum.drain(..=nl).collect();
                    line.pop(); // drop \n
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    *leftover = accum;
                    return Ok(line);
                }
            }
            None => {
                // EOF: return whatever we have. `Ok(Vec::new())`
                // signals EOF to the completion handler.
                return Ok(accum);
            }
        }
    }
}

fn read_n_with_cancel(
    shutdown_read_fd: RawFd,
    count: usize,
    leftover: &mut Vec<u8>,
) -> Result<Vec<u8>, String> {
    // Drain leftover bytes first. `port/read` returns "up to N bytes"
    // per POSIX semantics, so if leftover has anything we return that
    // immediately (matching how std::io::stdin().lock().read worked:
    // a single read syscall).
    if !leftover.is_empty() {
        let take = leftover.len().min(count);
        let chunk: Vec<u8> = leftover.drain(..take).collect();
        return Ok(chunk);
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    match read_chunk_with_cancel(shutdown_read_fd, count)? {
        Some(bytes) => Ok(bytes),
        None => Ok(Vec::new()), // EOF
    }
}

fn read_all_with_cancel(
    shutdown_read_fd: RawFd,
    leftover: &mut Vec<u8>,
) -> Result<Vec<u8>, String> {
    let mut accum = std::mem::take(leftover);
    loop {
        match read_chunk_with_cancel(shutdown_read_fd, 4096)? {
            Some(bytes) => accum.extend_from_slice(&bytes),
            None => return Ok(accum),
        }
    }
}

/// Blocking read on an inotify fd (Linux/Android).
#[cfg(any(target_os = "linux", target_os = "android"))]
fn watch_read_blocking(fd: RawFd) -> (i32, Vec<u8>) {
    let mut buf = vec![0u8; 4096];
    let ret = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
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

/// Blocking read on a signalfd (Linux). Reads up to 8 signalfd_siginfo
/// structs per call; signalfd has no shutdown(2)-equivalent, so a cancel
/// triggered by the scheduler closes the signalfd, which makes this
/// read return 0 (EOF) and the receiver is dead.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn sigfd_read_blocking(fd: RawFd) -> (i32, Vec<u8>) {
    use crate::io::sigfd::posix_trace;
    // The signalfd is created with SFD_NONBLOCK (see src/io/sigfd.rs) so the
    // io_uring path can rely on the kernel's poll-then-read pipeline. The
    // threadpool path has no such pipeline: a bare read(2) on a non-blocking
    // fd before the signal arrives returns -1/EAGAIN. Wait for POLLIN first,
    // looping on EINTR. POLLHUP on signalfd is unusual (no shutdown(2)
    // analogue) but we treat it as EOF for parity with WatchRead.
    posix_trace(format_args!("linux: sigfd_read_blocking entered fd={}", fd));
    let entry_size = std::mem::size_of::<libc::signalfd_siginfo>();
    loop {
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        posix_trace(format_args!(
            "linux: sigfd_read_blocking poll(fd={}, POLLIN, -1)",
            fd
        ));
        let pret = unsafe { libc::poll(&mut pfd, 1, -1) };
        if pret < 0 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
            posix_trace(format_args!(
                "linux: sigfd_read_blocking poll errno={}",
                errno
            ));
            if errno == libc::EINTR {
                continue;
            }
            return (-errno, Vec::new());
        }
        posix_trace(format_args!(
            "linux: sigfd_read_blocking poll returned, revents=0x{:x}",
            pfd.revents
        ));
        let mut buf = vec![0u8; entry_size * 8];
        let ret = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if ret < 0 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
            posix_trace(format_args!(
                "linux: sigfd_read_blocking read errno={}",
                errno
            ));
            // Racy: a different reader may have drained the queue between
            // poll and read. Loop back to poll.
            if errno == libc::EAGAIN || errno == libc::EWOULDBLOCK || errno == libc::EINTR {
                continue;
            }
            return (-errno, Vec::new());
        }
        buf.truncate(ret as usize);
        posix_trace(format_args!(
            "linux: sigfd_read_blocking returning n={}",
            ret
        ));
        return (ret as i32, buf);
    }
}

/// Blocking kevent() on a kqueue fd registered with EVFILT_SIGNAL (macOS).
/// Encodes results as (signum:i32, count:u32) LE pairs for
/// SignalReceiver::parse_events().
///
/// `signals` is the set the receiver registered with kqueue. The worker
/// `pthread_sigmask`-UNBLOCKs them on itself before kevent() so the
/// kernel can pick this thread as the delivery target — kqueue's
/// `EVFILT_SIGNAL` is driven by the in-kernel delivery path, not by
/// signal generation. With every other thread in the process blocking
/// the signal (the threadpool worker default + the main thread's
/// `os/sig-watch` mask), parking SIGUSR1 on the process pending list
/// without any unblocked thread leaves no delivery path and the knote
/// never activates — exactly the macOS hang
/// `tests/elle/posix.lisp` test #1 was timing out on.
///
/// `SignalReceiver::new` installs a process-wide no-op sigaction
/// handler for each watched signal at refcount 0 → 1 (and restores at
/// 1 → 0) so that the delivery the kernel makes to this worker is a
/// harmless return-through-the-trampoline rather than the default
/// disposition (Term for SIGUSR1, etc.).
#[cfg(target_os = "macos")]
fn kq_sig_read_blocking(kq: RawFd, signals: &[libc::c_int]) -> (i32, Vec<u8>) {
    use crate::io::sigfd::posix_trace;
    posix_trace(format_args!(
        "macos: kq_sig_read_blocking entered kq={} signals={:?}",
        kq, signals
    ));
    // Unblock the watched signals on this thread for the lifetime of the
    // worker. The worker is single-use (each PoolOp spawns a fresh
    // thread that exits after sending the completion), so we don't
    // bother restoring on return.
    let mut to_unblock: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe { libc::sigemptyset(&mut to_unblock) };
    for &s in signals {
        unsafe { libc::sigaddset(&mut to_unblock, s) };
    }
    let unblock_ret =
        unsafe { libc::pthread_sigmask(libc::SIG_UNBLOCK, &to_unblock, std::ptr::null_mut()) };
    posix_trace(format_args!(
        "macos: kq_sig_read_blocking pthread_sigmask SIG_UNBLOCK ret={}",
        unblock_ret
    ));

    loop {
        let mut eventlist: [libc::kevent; 32] = unsafe { std::mem::zeroed() };
        posix_trace(format_args!(
            "macos: kq_sig_read_blocking calling kevent(kq={})",
            kq
        ));
        let n = unsafe {
            libc::kevent(
                kq,
                std::ptr::null(),
                0,
                eventlist.as_mut_ptr(),
                eventlist.len() as i32,
                std::ptr::null(),
            )
        };
        if n < 0 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
            posix_trace(format_args!(
                "macos: kq_sig_read_blocking kevent returned -1, errno={}",
                errno
            ));
            // If the no-op handler ran without SA_RESTART (or some other
            // signal interrupted kevent), retry. The knote state is
            // preserved across EINTR, so a subsequent kevent picks up
            // the same event.
            if errno == libc::EINTR {
                continue;
            }
            return (-errno, Vec::new());
        }
        posix_trace(format_args!(
            "macos: kq_sig_read_blocking kevent returned n={} events",
            n
        ));
        let mut data = Vec::with_capacity(n as usize * 8);
        for event in &eventlist[..n as usize] {
            let signum = event.ident as i32;
            let count = event.data as u32;
            posix_trace(format_args!(
                "macos: kq_sig_read_blocking event signum={} count={}",
                signum, count
            ));
            data.extend_from_slice(&signum.to_le_bytes());
            data.extend_from_slice(&count.to_le_bytes());
        }
        return (data.len() as i32, data);
    }
}

/// Blocking kevent() on a kqueue fd (macOS). Encodes results as
/// (fd:i32, fflags:u32) LE pairs for FsWatcher::parse_events().
#[cfg(target_os = "macos")]
fn watch_read_blocking(kq: RawFd) -> (i32, Vec<u8>) {
    let mut eventlist: [libc::kevent; 32] = unsafe { std::mem::zeroed() };
    let n = unsafe {
        libc::kevent(
            kq,
            std::ptr::null(),
            0,
            eventlist.as_mut_ptr(),
            eventlist.len() as i32,
            std::ptr::null(),
        )
    };
    if n < 0 {
        return (
            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
            Vec::new(),
        );
    }
    let mut data = Vec::with_capacity(n as usize * 8);
    for event in &eventlist[..n as usize] {
        data.extend_from_slice(&(event.ident as i32).to_le_bytes());
        data.extend_from_slice(&event.fflags.to_le_bytes());
    }
    (data.len() as i32, data)
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
        // ProcessWait encodes the exit code in data (LE i32), not result_code.
        assert_eq!(completions[0].result_code, 0, "waitpid should succeed");
        let exit_code = i32::from_le_bytes(completions[0].data[..4].try_into().unwrap());
        assert_eq!(exit_code, 0, "expected exit code 0 from /bin/true");
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
        // ProcessWait encodes the exit code in data (LE i32), not result_code.
        // result_code=0 means waitpid succeeded; the process exit code is in data.
        assert_eq!(completions[0].result_code, 0, "waitpid should succeed");
        let exit_code = i32::from_le_bytes(completions[0].data[..4].try_into().unwrap());
        assert_ne!(exit_code, 0, "expected non-zero exit code from /bin/false");
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

    /// Regression test for the macOS `EVFILT_SIGNAL` hang that prevented
    /// tests/elle/posix.lisp from passing on macOS, and a counter-factual
    /// guard for the Linux signalfd EAGAIN fix from commit f7aed410.
    ///
    /// Forks a child process so we get a clean thread topology that
    /// mirrors production: only the main thread plus our intentionally-
    /// spawned threadpool worker, all with the watched signal masked.
    /// In the cargo test runner this isn't true — peer test threads have
    /// SIGUSR1 unmasked and would absorb the `kill()` before our
    /// signalfd/kqueue worker reads it.
    ///
    /// Child flow:
    ///   1. Open a `SignalReceiver` for SIGUSR1 (blocks it on this
    ///      thread; the threadpool worker spawned in step 2 inherits the
    ///      mask).
    ///   2. Submit the platform's blocking signal-read op (`SigfdRead` on
    ///      Linux, `KqSigRead` on macOS) — the same threadpool primitive
    ///      `submit_sig_next` uses in production.
    ///   3. `kill(getpid(), SIGUSR1)` from the main thread.
    ///   4. Wait up to 5 s for a completion; assert it parses to a
    ///      single SIGUSR1 event.
    ///
    /// Child exits 0 on success, a small positive code on failure.
    ///
    /// On macOS this gates the fix: kqueue's `EVFILT_SIGNAL` fires from
    /// the in-kernel delivery path, so if every thread in the process
    /// blocks the signal the kernel parks it on the process pending list
    /// and the knote is never activated. Without the fix the child hangs
    /// past the parent's wait timeout (waitpid loop bounded at 10 s).
    #[test]
    fn sig_read_returns_after_kill_to_self() {
        use std::time::{Duration, Instant};

        let pid = unsafe { libc::fork() };
        if pid < 0 {
            panic!("fork failed: {}", std::io::Error::last_os_error());
        }
        if pid == 0 {
            // CHILD: run the test logic and _exit. Use _exit to skip
            // atexit/destructors — Rust drop glue across the fork
            // boundary is unsupported in general.
            let code = sig_read_child_logic();
            unsafe { libc::_exit(code) };
        }

        // PARENT: bounded waitpid so a regression surfaces fast instead
        // of wedging the test runner.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut status: libc::c_int = 0;
        loop {
            let wret = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if wret == pid {
                break;
            }
            if wret < 0 {
                let errno = std::io::Error::last_os_error();
                panic!("waitpid({}): {}", pid, errno);
            }
            if Instant::now() >= deadline {
                // Kill the child so we don't leak the process and panic
                // with a meaningful message.
                unsafe { libc::kill(pid, libc::SIGKILL) };
                let _ = unsafe { libc::waitpid(pid, &mut status, 0) };
                panic!("sig_read child hung past 10s");
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        if libc::WIFSIGNALED(status) {
            panic!("sig_read child died from signal {}", libc::WTERMSIG(status));
        }
        let code = libc::WEXITSTATUS(status);
        assert_eq!(
            code, 0,
            "sig_read child failed with code {} (see codes in sig_read_child_logic)",
            code
        );
    }

    /// Body of the forked child for `sig_read_returns_after_kill_to_self`.
    /// Returns a small positive exit code identifying which step failed,
    /// or 0 on success. Kept narrow on purpose: no allocations between
    /// fork and the kernel calls beyond what `SignalReceiver` and
    /// `ThreadPoolBackend` already do.
    fn sig_read_child_logic() -> i32 {
        use crate::io::sigfd::SignalReceiver;
        use std::time::Duration;

        let r = match SignalReceiver::new(vec![libc::SIGUSR1]) {
            Ok(r) => r,
            Err(_) => return 11,
        };
        let fd = match r.raw_fd() {
            Ok(f) => f,
            Err(_) => return 12,
        };

        let mut pool = ThreadPoolBackend::new();
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let submit = pool.submit(1, PoolOp::SigfdRead { fd });
        #[cfg(target_os = "macos")]
        let submit = pool.submit(
            1,
            PoolOp::KqSigRead {
                fd,
                signals: vec![libc::SIGUSR1],
            },
        );
        if submit.is_err() {
            return 13;
        }

        // Let the worker enter the blocking syscall first — matches the
        // (ev/sleep 0.05) preamble in tests/elle/posix.lisp test #1.
        std::thread::sleep(Duration::from_millis(50));

        if unsafe { libc::kill(libc::getpid(), libc::SIGUSR1) } != 0 {
            return 14;
        }

        let completions = match pool.wait(Some(5000)) {
            Ok(c) => c,
            Err(_) => return 15,
        };
        if completions.is_empty() {
            return 16;
        }
        let pc = &completions[0];
        if pc.result_code <= 0 {
            return 17;
        }
        let events = r.parse_events(&pc.data[..pc.result_code as usize]);
        if events.is_empty() {
            return 18;
        }
        if events[0].signum != libc::SIGUSR1 {
            return 19;
        }
        r.close();
        0
    }

    /// Regression test for the macOS test 5 failure mode: after the
    /// kqueue worker reports the event for a `kill(getpid(), SIGUSR1)`,
    /// macOS leaves an instance of the signal in the process pending
    /// queue (EVFILT_SIGNAL counts kill() generations on the knote but
    /// does not consume from the pending queue, and the worker's brief
    /// SIG_UNBLOCK + no-op handler delivery only drains at most one
    /// instance). Before the `rollback`-time drain
    /// (src/io/sigfd.rs::drain_pending_blocked) `os/sig-close` would
    /// restore the SIGUSR1 default disposition (Term) and then
    /// `pthread_sigmask(SIG_UNBLOCK, …)`, firing the pending Term on
    /// the closing thread and killing the process mid-close — exactly
    /// the silent death observed at `test 5: pre-close` in
    /// tests/elle/posix.lisp on macOS CI.
    ///
    /// This test reproduces the shape (two kills, one read, close)
    /// inside a forked child and asserts the child exits 0 rather
    /// than dying from signal 10/SIGUSR1.
    #[test]
    fn close_drains_pending_after_two_kills() {
        use std::time::{Duration, Instant};

        let pid = unsafe { libc::fork() };
        if pid < 0 {
            panic!("fork failed: {}", std::io::Error::last_os_error());
        }
        if pid == 0 {
            let code = close_drain_child_logic();
            unsafe { libc::_exit(code) };
        }

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut status: libc::c_int = 0;
        loop {
            let wret = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if wret == pid {
                break;
            }
            if wret < 0 {
                let errno = std::io::Error::last_os_error();
                panic!("waitpid({}): {}", pid, errno);
            }
            if Instant::now() >= deadline {
                unsafe { libc::kill(pid, libc::SIGKILL) };
                let _ = unsafe { libc::waitpid(pid, &mut status, 0) };
                panic!("close_drains_pending child hung past 10s");
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        if libc::WIFSIGNALED(status) {
            let sig = libc::WTERMSIG(status);
            panic!(
                "close_drains_pending child died from signal {} \
                 (expected clean exit; pending signal at close-time \
                 unblock was NOT drained)",
                sig
            );
        }
        let code = libc::WEXITSTATUS(status);
        assert_eq!(
            code, 0,
            "close_drains_pending child failed with code {} (see codes in close_drain_child_logic)",
            code
        );
    }

    /// Shutdown signal must wake the stdin thread when it is sitting in
    /// `request_rx.recv()` waiting for the next request (no read in
    /// flight). The thread should exit cleanly within a short
    /// timeout. Counter-factual: without the self-pipe + shutdown
    /// wiring, the thread sits in `recv()` until the channel sender
    /// drops, which doesn't happen until process exit — the test
    /// helper's `recv_timeout` below would fire.
    ///
    /// This test does NOT need to touch fd 0. The thread is idle
    /// (never submits a request) so the read syscall is never reached.
    #[test]
    fn stdin_thread_shutdown_while_idle_joins() {
        use std::time::{Duration, Instant};
        let st = StdinThread::new();
        st.shutdown();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !st.is_finished() {
            if Instant::now() >= deadline {
                panic!("stdin thread did not exit within 2s of shutdown");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Shutdown signal must wake the stdin thread when it is parked
    /// inside `libc::read(0, …)` waiting for input. We fork so we can
    /// `dup2` a pipe onto fd 0 in the child without disturbing the
    /// cargo test runner (peer tests share fd 0). The child holds the
    /// write end open so the read truly blocks (no EOF). After a 100 ms
    /// settle, the child calls `shutdown()` and expects an error
    /// completion within 2 s.
    ///
    /// Counter-factual: the legacy
    /// `std::io::stdin().lock().read_line(…)` auto-retries on EINTR
    /// and has no shutdown path; a signal or pipe-write cannot wake
    /// it. The forked child would hang past the 5 s parent timeout
    /// and panic.
    #[test]
    fn stdin_thread_shutdown_cancels_inflight_read() {
        use std::time::{Duration, Instant};
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            panic!("fork: {}", std::io::Error::last_os_error());
        }
        if pid == 0 {
            unsafe { libc::_exit(stdin_close_child_logic()) };
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut status: libc::c_int = 0;
        loop {
            let wret = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if wret == pid {
                break;
            }
            if wret < 0 {
                panic!("waitpid: {}", std::io::Error::last_os_error());
            }
            if Instant::now() >= deadline {
                unsafe { libc::kill(pid, libc::SIGKILL) };
                let _ = unsafe { libc::waitpid(pid, &mut status, 0) };
                panic!("stdin close child hung past 5s");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !libc::WIFSIGNALED(status),
            "child died from signal {}",
            libc::WTERMSIG(status),
        );
        assert!(libc::WIFEXITED(status));
        assert_eq!(
            libc::WEXITSTATUS(status),
            0,
            "child exited with {} (see codes 51-58 in stdin_close_child_logic)",
            libc::WEXITSTATUS(status)
        );
    }

    fn stdin_close_child_logic() -> i32 {
        use std::time::Duration;
        // Replace fd 0 with the read end of a pipe and hold the write
        // end so the read never sees EOF. The stdin thread will block
        // inside libc::read(0, …) until our shutdown signal wakes it.
        let mut pipe_fds: [libc::c_int; 2] = [0; 2];
        if unsafe { libc::pipe(pipe_fds.as_mut_ptr()) } != 0 {
            return 51;
        }
        if unsafe { libc::dup2(pipe_fds[0], 0) } < 0 {
            return 52;
        }
        unsafe { libc::close(pipe_fds[0]) };
        let _write_end = pipe_fds[1]; // kept open until process exit

        let st = StdinThread::new();
        if st.submit(1, StdinOpKind::ReadLine).is_err() {
            return 53;
        }
        // Settle: give the thread time to enter the read.
        std::thread::sleep(Duration::from_millis(100));

        st.shutdown();

        match st.receiver().recv_timeout(Duration::from_secs(2)) {
            Ok(c) => {
                if c.id != 1 {
                    return 54;
                }
                match c.result {
                    Ok(_) => 55, // expected an error, got Ok
                    Err(msg) => {
                        if msg.contains("stdin closed") {
                            0
                        } else {
                            56
                        }
                    }
                }
            }
            Err(_) => 57,
        }
    }

    /// Body of the forked child for `close_drains_pending_after_two_kills`.
    /// Reads ONE signal via sig-next (proving the watcher works), then
    /// raises SIGUSR1 AGAIN with no reader pending so the signal sits in
    /// the kernel queue at close time. The drain in rollback must
    /// consume it; otherwise close's post-restore unblock fires the
    /// default disposition (Term for SIGUSR1) on the calling thread and
    /// kills us. Reaching `return 0` after `r.close()` IS the test.
    ///
    /// This reproduces on both Linux and macOS:
    ///  - Linux: signalfd dequeues at read time, so the post-read kill
    ///    is what leaves something stuck in the queue at close.
    ///  - macOS: the EVFILT_SIGNAL knote never dequeues from the
    ///    process pending queue, so the original kill ALSO survives —
    ///    but the post-read kill is the portable trigger.
    fn close_drain_child_logic() -> i32 {
        use crate::io::sigfd::SignalReceiver;
        use std::time::Duration;

        let r = match SignalReceiver::new(vec![libc::SIGUSR1]) {
            Ok(r) => r,
            Err(_) => return 21,
        };
        let fd = match r.raw_fd() {
            Ok(f) => f,
            Err(_) => return 22,
        };

        // First kill + sig-next round-trip. Proves the watcher works
        // and consumes one pending instance through the kernel.
        if unsafe { libc::kill(libc::getpid(), libc::SIGUSR1) } != 0 {
            return 23;
        }
        let mut pool = ThreadPoolBackend::new();
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let submit = pool.submit(1, PoolOp::SigfdRead { fd });
        #[cfg(target_os = "macos")]
        let submit = pool.submit(
            1,
            PoolOp::KqSigRead {
                fd,
                signals: vec![libc::SIGUSR1],
            },
        );
        if submit.is_err() {
            return 24;
        }
        let completions = match pool.wait(Some(5000)) {
            Ok(c) => c,
            Err(_) => return 25,
        };
        if completions.is_empty() {
            return 26;
        }
        if completions[0].result_code <= 0 {
            return 27;
        }

        // SECOND kill — no reader pending. The signal sits in the
        // process pending queue (SIGUSR1 still blocked on this thread
        // from SignalReceiver::new). On close, without the drain in
        // rollback the pthread_sigmask SIG_UNBLOCK fires the
        // about-to-be-restored SIGUSR1 default (Term) and the child
        // dies from signal 10 — observable as WIFSIGNALED=true,
        // WTERMSIG=SIGUSR1 in the parent.
        if unsafe { libc::kill(libc::getpid(), libc::SIGUSR1) } != 0 {
            return 28;
        }
        // Brief sleep so the kill is definitely queued before close.
        std::thread::sleep(Duration::from_millis(10));

        // The smoking-gun call. With the drain it returns; without it
        // the process dies here.
        r.close();
        std::thread::sleep(Duration::from_millis(10));
        0
    }
}
