use super::*;

impl CompletionHub {
    /// Submit a blocking I/O operation on a background worker thread; the worker
    /// reports its result back through the hub channel as a `RawCompletion::Pool`.
    pub(in crate::io) fn submit(&mut self, id: SubmissionId, op: PoolOp) -> Result<(), String> {
        // The pool carries the id as an opaque round-tripped token.
        let id = id.as_u64();
        if self.in_flight() >= MAX_THREAD_POOL_OPS {
            return Err("async I/O: too many concurrent operations (max 64)".into());
        }
        let sender = self.sender();
        let eventfd = self.eventfd();
        self.note_submit();
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
        Ok(())
    }
}
