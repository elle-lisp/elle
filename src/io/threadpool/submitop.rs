use super::*;
use std::time::Instant;

impl CompletionHub {
    /// Submit a blocking I/O operation on a background worker thread; the worker
    /// reports its result back through the hub channel as a `RawCompletion::Pool`.
    /// How many operations may run at once is the OS's to say. The worker is
    /// started with `Builder::spawn` rather than `thread::spawn` for exactly
    /// that reason: `thread::spawn` panics when the OS refuses a thread, and a
    /// refusal is something the calling fiber can be told about and handle.
    /// So the ceiling here is `RLIMIT_NPROC`, `threads-max` and the memory for
    /// the stacks — the limits the operator set — reported where they bind
    /// rather than guessed at in advance.
    pub(in crate::io) fn submit(&mut self, id: SubmissionId, op: PoolOp) -> Result<(), String> {
        // Taken before `op` moves into the worker: a spawn that fails leaves
        // this operation's stop pipe with no worker to own its read end.
        let stop_fd = op.stop_fd();
        // The pool carries the id as an opaque round-tripped token.
        let raw_id = id.as_u64();
        let sender = self.sender();
        let eventfd = self.eventfd();
        let started = std::thread::Builder::new().spawn(move || {
            let id = raw_id;
            // Block all signals on this worker so the kernel never selects
            // it as the delivery target for a watched POSIX signal.
            // See src/io/sigfd.rs and docs/posix-signals.md.
            crate::io::sigfd::mask_all_signals_on_this_thread();
            let (result_code, data) = match op {
                PoolOp::Read {
                    fd,
                    size,
                    timeout,
                    stop,
                } => {
                    let bound = OpBound::new(fd, timeout, stop);
                    let mut buf = vec![0u8; size];
                    loop {
                        let ret =
                            unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, size) };
                        if ret >= 0 {
                            buf.truncate(ret as usize);
                            break (ret as i32, buf);
                        }
                        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
                        if errno == libc::EINTR {
                            continue;
                        }
                        if is_would_block(errno) {
                            match bound.wait(libc::POLLIN) {
                                Wake::Ready => continue,
                                Wake::Stopped => break (-libc::ECANCELED, Vec::new()),
                                Wake::TimedOut => break (-libc::ETIMEDOUT, Vec::new()),
                            }
                        }
                        break (-errno, Vec::new());
                    }
                }
                PoolOp::ReadExact {
                    fd,
                    size,
                    graphemes,
                    gen,
                    timeout,
                    stop,
                } => {
                    let bound = OpBound::new(fd, timeout, stop);
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
                            let g = grapheme_count_in_valid_prefix(&buf[..total], gen);
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
                            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
                            if errno == libc::EINTR {
                                continue;
                            }
                            if is_would_block(errno) {
                                match bound.wait(libc::POLLIN) {
                                    Wake::Ready => continue,
                                    Wake::Stopped => break (-libc::ECANCELED, Vec::new()),
                                    // The deadline passed with nothing more
                                    // arriving. That is the caller's timeout,
                                    // not the end of the stream — surfacing
                                    // the partial here would read as EOF and
                                    // the completion would map it to nil.
                                    Wake::TimedOut => break (-libc::ETIMEDOUT, Vec::new()),
                                }
                            }
                            if total == 0 {
                                break (-errno, Vec::new());
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
                PoolOp::ReadLine { fd, timeout, stop } | PoolOp::ReadAll { fd, timeout, stop } => {
                    let until_newline = matches!(op, PoolOp::ReadLine { .. });
                    let bound = OpBound::new(fd, timeout, stop);
                    let mut accumulated = Vec::new();
                    let mut chunk = vec![0u8; 4096];
                    loop {
                        let ret = unsafe {
                            libc::read(fd, chunk.as_mut_ptr() as *mut libc::c_void, chunk.len())
                        };
                        if ret < 0 {
                            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
                            if errno == libc::EINTR {
                                continue;
                            }
                            if is_would_block(errno) {
                                match bound.wait(libc::POLLIN) {
                                    Wake::Ready => continue,
                                    Wake::Stopped => break (-libc::ECANCELED, Vec::new()),
                                    // The deadline passed with nothing more
                                    // arriving. Report the timeout rather than
                                    // the partial, which the completion would
                                    // treat as a line or a stream that ended.
                                    Wake::TimedOut => break (-libc::ETIMEDOUT, Vec::new()),
                                }
                            }
                            if accumulated.is_empty() {
                                break (-errno, Vec::new());
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
                PoolOp::Write {
                    fd,
                    data,
                    timeout,
                    stop,
                } => {
                    // `port/write` writes every byte before it returns
                    // (docs/io.md), so loop until the payload is gone. One
                    // write(2) transfers only what fits in the fd's send
                    // buffer, which on a socket is routinely a fraction of a
                    // large payload.
                    //
                    // The caller's timeout bounds every pass of this loop, not
                    // the call: a peer that has stopped reading trips one
                    // wait, while one that merely reads slowly keeps making
                    // progress and the transfer finishes however long it
                    // takes. That mirrors the io_uring path, which re-arms its
                    // LinkTimeout on each resubmission.
                    let bound = OpBound::new(fd, timeout, stop);
                    let mut total = 0usize;
                    loop {
                        let ret = unsafe {
                            libc::write(
                                fd,
                                data[total..].as_ptr() as *const libc::c_void,
                                data.len() - total,
                            )
                        };
                        if ret > 0 {
                            total += ret as usize;
                            if total >= data.len() {
                                break (total as i32, Vec::new());
                            }
                            continue;
                        }
                        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
                        if ret < 0 && errno == libc::EINTR {
                            // A signal interrupted the syscall before any byte
                            // moved; the payload is unchanged, so retry.
                            continue;
                        }
                        if ret < 0 && is_would_block(errno) {
                            match bound.wait(libc::POLLOUT) {
                                Wake::Ready => continue,
                                Wake::Stopped => break (-libc::ECANCELED, Vec::new()),
                                // The wait for room expired. Report it as the
                                // caller's timeout, which `complete_port_op`
                                // maps to a `:timeout` error rather than a
                                // generic I/O one.
                                Wake::TimedOut => break (-libc::ETIMEDOUT, Vec::new()),
                            }
                        }
                        // Surface the failure rather than the bytes that did
                        // get through: a count smaller than the payload reads
                        // as a completed write to a caller that trusts the
                        // full-write contract. A zero return on a non-empty
                        // tail cannot make progress either, so it fails too.
                        break (-(if ret == 0 { libc::EIO } else { errno }), Vec::new());
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
                PoolOp::Accept { fd, timeout, stop } => {
                    let bound = OpBound::new(fd, timeout, stop);
                    let mut addr_storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
                    let mut addr_len: libc::socklen_t =
                        std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
                    let new_fd = take_when_ready(&bound, libc::POLLIN, || {
                        let mut len: libc::socklen_t =
                            std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
                        let r = unsafe {
                            libc::accept(
                                fd,
                                &mut addr_storage as *mut _ as *mut libc::sockaddr,
                                &mut len,
                            )
                        };
                        if r >= 0 {
                            addr_len = len;
                        }
                        r as isize
                    }) as i32;
                    if new_fd < 0 {
                        (new_fd, Vec::new())
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
                PoolOp::ConnectTcp {
                    addr,
                    options,
                    timeout,
                    stop,
                } => {
                    let family = if addr.is_ipv6() {
                        libc::AF_INET6
                    } else {
                        libc::AF_INET
                    };
                    let (sa, sa_len) = crate::io::sockaddr::build_inet(&addr);
                    connect_socket(
                        family,
                        sa.as_ptr() as *const libc::sockaddr,
                        sa_len,
                        &options,
                        timeout,
                        stop,
                        addr.to_string(),
                    )
                }
                PoolOp::ConnectUnix {
                    path,
                    options,
                    timeout,
                    stop,
                } => match crate::io::sockaddr::build_unix(&path) {
                    Err(msg) => {
                        // A path the kernel could never accept, caught before a
                        // descriptor is opened for it.
                        if let Some(fd) = stop {
                            unsafe { libc::close(fd) };
                        }
                        (-libc::EINVAL, msg.into_bytes())
                    }
                    Ok((sun, addr_len)) => connect_socket(
                        libc::AF_UNIX,
                        &sun as *const _ as *const libc::sockaddr,
                        addr_len,
                        &options,
                        timeout,
                        stop,
                        path.clone(),
                    ),
                },
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
                PoolOp::RecvFrom {
                    fd,
                    size,
                    timeout,
                    stop,
                } => {
                    let bound = OpBound::new(fd, timeout, stop);
                    let mut buf = vec![0u8; size];
                    let mut addr_storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
                    let mut addr_len: libc::socklen_t =
                        std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
                    let ret = take_when_ready(&bound, libc::POLLIN, || {
                        let mut len: libc::socklen_t =
                            std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
                        let r = unsafe {
                            libc::recvfrom(
                                fd,
                                buf.as_mut_ptr() as *mut libc::c_void,
                                buf.len(),
                                0,
                                &mut addr_storage as *mut _ as *mut libc::sockaddr,
                                &mut len,
                            )
                        };
                        if r >= 0 {
                            addr_len = len;
                        }
                        r
                    });
                    if ret < 0 {
                        (ret as i32, Vec::new())
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
                PoolOp::Sleep { nanos, stop } => {
                    // Either ending reports the same completion: a stopped
                    // timer's result is discarded, and a fiber that wanted the
                    // elapsed timer cannot tell the two apart anyway. There is
                    // no descriptor to watch, so the bound polls the stop pipe
                    // alone with the duration as its deadline.
                    let bound = OpBound::new(-1, Some(Duration::from_nanos(nanos)), stop);
                    let _ = bound.sleep();
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
                PoolOp::SigfdRead { fd, trace } => sigfd_read_blocking(&trace, fd),
                #[cfg(not(any(target_os = "linux", target_os = "android")))]
                PoolOp::SigfdRead { .. } => (
                    -libc::ENOTSUP,
                    b"sig-next: signalfd not supported on this platform".to_vec(),
                ),
                #[cfg(target_os = "macos")]
                PoolOp::KqSigRead { fd, signals, trace } => {
                    kq_sig_read_blocking(&trace, fd, &signals)
                }
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
            publish_completion(
                &sender,
                eventfd,
                RawCompletion::Pool(PoolCompletion {
                    id,
                    result_code,
                    data,
                }),
            );
        });
        match started {
            Ok(_) => {
                // Counted only once the worker exists, so a refused spawn
                // leaves nothing behind to reap. Nothing can reap between the
                // two either: the drain runs on this thread.
                self.note_submit();
                Ok(())
            }
            Err(e) => {
                // No worker took the stop pipe, so close both ends here rather
                // than leave the read end with no owner.
                if let Some(fd) = stop_fd {
                    // SAFETY: the read end is still this operation's, unshared.
                    unsafe { libc::close(fd) };
                }
                self.forget_stop(id);
                Err(format!("async I/O: cannot start a worker thread: {}", e))
            }
        }
    }
}

/// Take from a descriptor that may have nothing yet: wait for `events` under
/// the operation's bound, attempt the syscall, and repeat while the attempt
/// reports that nothing was there. Returns what `attempt` returned, or
/// `-ECANCELED` / `-ETIMEDOUT` for the two ways an operation ends without it.
///
/// The wait comes first because the syscall must never park. A worker inside a
/// blocking `accept(2)` or `recvfrom(2)` is unreachable — closing the socket
/// does not wake a thread already in the syscall — so the operation would
/// outlive both its deadline and the fiber that asked for it.
///
/// `OpBound` takes the descriptor non-blocking for the operation's lifetime, so
/// a readiness another operation consumed first reports `EAGAIN` instead of
/// parking. That and `EINTR` both mean "nothing taken yet": wait again rather
/// than report a failure.
fn take_when_ready(
    bound: &OpBound,
    events: libc::c_short,
    mut attempt: impl FnMut() -> isize,
) -> isize {
    loop {
        match bound.wait(events) {
            Wake::Stopped => return -(libc::ECANCELED as isize),
            Wake::TimedOut => return -(libc::ETIMEDOUT as isize),
            Wake::Ready => {}
        }
        let r = attempt();
        if r >= 0 {
            return r;
        }
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
        if !is_would_block(errno) && errno != libc::EINTR {
            return -(errno as isize);
        }
    }
}

/// How long a connect that reported `EAGAIN` waits before trying again.
/// AF_UNIX reports a listener whose backlog is full that way, and offers no
/// readiness to wait on, so that retry is paced rather than driven by an event.
const CONNECT_RETRY_PACE: Duration = Duration::from_millis(10);

/// Open a socket, connect it to `sa`, and report the connected descriptor —
/// or `-errno`. `label` describes the peer for the completion's data.
///
/// The descriptor belongs to this operation, so the bound can take it
/// non-blocking for the whole connect. That is what makes the connect
/// answerable: a blocking `connect(2)` holds this worker through the peer's
/// entire retry sequence, where neither the caller's deadline nor a
/// cancellation can reach it.
#[allow(clippy::too_many_arguments)]
fn connect_socket(
    family: libc::c_int,
    sa: *const libc::sockaddr,
    sa_len: libc::socklen_t,
    options: &SocketOptions,
    timeout: Option<Duration>,
    stop: Option<RawFd>,
    label: String,
) -> (i32, Vec<u8>) {
    let fd = unsafe { libc::socket(family, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
        // No bound was built, so nothing else owns the stop pipe's read end.
        if let Some(stop_fd) = stop {
            // SAFETY: the read end is this operation's, unshared.
            unsafe { libc::close(stop_fd) };
        }
        return (-errno, Vec::new());
    }
    unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    // Before the handshake, as the io_uring path does: a TCP window is
    // negotiated while the handshake runs.
    crate::io::request::apply_socket_options(fd, options);

    // The bound is dropped before the failure close below. It clears the
    // non-blocking flag through the descriptor number, which a closed
    // descriptor could have handed to another thread's socket by then.
    let outcome = {
        let bound = OpBound::new(fd, timeout, stop);
        connect_bounded(fd, sa, sa_len, &bound, timeout)
    };
    if outcome < 0 {
        unsafe { libc::close(fd) };
        return (outcome, Vec::new());
    }
    (fd, label.into_bytes())
}

/// Drive one non-blocking connect to its outcome under `bound`: 0 once the
/// peer answers, or `-errno` — `-ECANCELED` when stopped, `-ETIMEDOUT` at the
/// caller's deadline.
///
/// The deadline spans the whole connect rather than each retry, because a
/// connect is one operation however many times the kernel makes us ask.
fn connect_bounded(
    fd: RawFd,
    sa: *const libc::sockaddr,
    sa_len: libc::socklen_t,
    bound: &OpBound,
    timeout: Option<Duration>,
) -> i32 {
    let deadline = timeout.map(|t| Instant::now() + t);
    loop {
        if unsafe { libc::connect(fd, sa, sa_len) } == 0 {
            return 0;
        }
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
        match errno {
            // The handshake is under way, and reports its outcome by making
            // the socket writable.
            libc::EINPROGRESS | libc::EALREADY | libc::EINTR => {
                return match bound.wait(libc::POLLOUT) {
                    Wake::Ready => connect_result(fd),
                    Wake::Stopped => -libc::ECANCELED,
                    Wake::TimedOut => -libc::ETIMEDOUT,
                }
            }
            // A second call after the connection is up says so this way.
            libc::EISCONN => return 0,
            // The peer's backlog is full (AF_UNIX). There is no readiness to
            // wait for, so pace the retry — the pause still watches the stop.
            libc::EAGAIN => {
                let slice = match deadline {
                    None => CONNECT_RETRY_PACE,
                    Some(at) => {
                        let left = at.saturating_duration_since(Instant::now());
                        if left.is_zero() {
                            return -libc::ETIMEDOUT;
                        }
                        left.min(CONNECT_RETRY_PACE)
                    }
                };
                if matches!(bound.pause(slice), Wake::Stopped) {
                    return -libc::ECANCELED;
                }
            }
            e => return -e,
        }
    }
}

/// The outcome of a connect that reported writability, read from the socket's
/// own error slot — where a handshake that failed leaves its reason.
fn connect_result(fd: RawFd) -> i32 {
    let mut err: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    let got = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_ERROR,
            &mut err as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if got < 0 {
        return -std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
    }
    if err == 0 {
        0
    } else {
        -err
    }
}
