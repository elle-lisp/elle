use super::*;

/// Submit a stream I/O operation (Read, ReadLine, ReadAll, Write, Flush).
///
/// `read_buffered`: for Read ops, the number of bytes already sitting in the
/// fd_state buffer. The kernel read is reduced by this amount so the
/// completion handler can prepend the buffered prefix.
#[allow(clippy::too_many_arguments)]
pub(crate) fn submit_uring_stream(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    fd: RawFd,
    op: &IoOp,
    timeout: Option<Duration>,
    buffer_pool: &mut BufferPool,
    buf_handle: Option<BufferHandle>,
    read_buffered: usize,
) -> Result<(), String> {
    use io_uring::opcode;
    use io_uring::types::Fd;

    let entry = match op {
        IoOp::ReadLine { buffer } => {
            let (dst, dst_cap) = unsafe { crate::io::request::writeable_buffer_ptr(buffer) };
            let read_size = (dst_cap - read_buffered).min(4096);
            unsafe {
                opcode::Read::new(Fd(fd), dst.add(read_buffered), read_size as u32)
                    .offset(u64::MAX)
                    .build()
                    .user_data(id.as_u64())
            }
        }
        IoOp::ReadAll => {
            let bh = buf_handle.expect("ReadAll requires BufferHandle");
            let buf = buffer_pool.get_mut(bh);
            buf.resize(4096, 0);
            opcode::Read::new(Fd(fd), buf.as_mut_ptr(), buf.len() as u32)
                .offset(u64::MAX)
                .build()
                .user_data(id.as_u64())
        }
        IoOp::Read { count, buffer } | IoOp::ReadExact { count, buffer } => {
            let (dst, dst_cap) = unsafe { crate::io::request::writeable_buffer_ptr(buffer) };
            // Fill to buffer capacity. For binary Read/ReadExact the buffer
            // is sized to `count`, so this reads exactly `count` bytes. For a
            // text ReadExact the buffer is oversized (4 bytes/grapheme), so
            // this reads as many bytes as the buffer holds in one shot — the
            // gate then stops once `count` graphemes are assembled and the
            // completion splits + stashes the remainder.
            let _ = count;
            let read_size = dst_cap.saturating_sub(read_buffered).min(MAX_READ_CHUNK);
            unsafe {
                opcode::Read::new(Fd(fd), dst.add(read_buffered), read_size as u32)
                    .offset(u64::MAX)
                    .build()
                    .user_data(id.as_u64())
            }
        }
        IoOp::Write { data } => {
            let bytes = crate::io::aio::AsyncBackend::extract_write_bytes(data);
            let bh = buf_handle.expect("Write requires BufferHandle");
            let buf = buffer_pool.get_mut(bh);
            buf.clear();
            buf.extend_from_slice(&bytes);
            // The whole payload stays in the pooled buffer: the fd accepts only
            // what fits in its send buffer, and `drain_cqes` resubmits the tail
            // from here until every byte is gone.
            let write_size = buf.len().min(MAX_WRITE_CHUNK);
            opcode::Write::new(Fd(fd), buf.as_ptr(), write_size as u32)
                .offset(u64::MAX)
                .build()
                .user_data(id.as_u64())
        }
        IoOp::Flush => opcode::Fsync::new(Fd(fd)).build().user_data(id.as_u64()),
        _ => return Err(format!("io/submit: unexpected stream op {:?}", op)),
    };

    let entry = if timeout.is_some() {
        entry.flags(io_uring::squeue::Flags::IO_LINK)
    } else {
        entry
    };

    unsafe {
        ring.submission()
            .push(&entry)
            .map_err(|_| "io/submit: io_uring submission queue full".to_string())?;
    }

    if let Some(dur) = timeout {
        let ts = io_uring::types::Timespec::new()
            .sec(dur.as_secs())
            .nsec(dur.subsec_nanos());
        let timeout_sqe = opcode::LinkTimeout::new(&ts)
            .build()
            .user_data(id.as_u64() | TIMEOUT_USER_DATA_TAG);
        unsafe {
            ring.submission()
                .push(&timeout_sqe)
                .map_err(|_| "io/submit: io_uring submission queue full".to_string())?;
        }
    }

    ring.submit()
        .map_err(|e| format!("io/submit: io_uring submit failed: {}", e))?;
    Ok(())
}
pub(crate) fn submit_uring_accept(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    fd: RawFd,
    timeout: Option<Duration>,
) -> Result<(), String> {
    use io_uring::opcode;
    use io_uring::types::Fd;

    let accept_sqe = opcode::Accept::new(Fd(fd), std::ptr::null_mut(), std::ptr::null_mut())
        .build()
        .user_data(id.as_u64());

    let accept_sqe = if timeout.is_some() {
        accept_sqe.flags(io_uring::squeue::Flags::IO_LINK)
    } else {
        accept_sqe
    };

    unsafe {
        ring.submission()
            .push(&accept_sqe)
            .map_err(|_| "io/submit: io_uring submission queue full".to_string())?;
    }

    if let Some(dur) = timeout {
        let ts = io_uring::types::Timespec::new()
            .sec(dur.as_secs())
            .nsec(dur.subsec_nanos());
        let timeout_sqe = opcode::LinkTimeout::new(&ts)
            .build()
            .user_data(id.as_u64() | TIMEOUT_USER_DATA_TAG);
        unsafe {
            ring.submission()
                .push(&timeout_sqe)
                .map_err(|_| "io/submit: io_uring submission queue full".to_string())?;
        }
    }

    ring.submit()
        .map_err(|e| format!("io/submit: io_uring submit failed: {}", e))?;
    Ok(())
}
/// Submit a Connect SQE via io_uring.
///
/// Creates a non-blocking socket, builds the sockaddr, and submits
/// `opcode::Connect`. The socket fd is returned so the caller can stash it
/// in `PendingOp.connect_fd`. On CQE success (result_code == 0), that fd
/// is the connected socket.
pub(crate) fn submit_uring_connect(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    addr: &ConnectAddr,
    timeout: Option<Duration>,
    buffer_pool: &mut BufferPool,
    buf_handle: BufferHandle,
) -> Result<RawFd, String> {
    use io_uring::opcode;
    use io_uring::types::Fd;

    let (sock_fd, sockaddr_buf, sockaddr_len) = match addr {
        ConnectAddr::Tcp {
            addr: ip,
            port: port_num,
            ..
        } => {
            // IP is already parsed (the primitive narrows to IP; the stdlib
            // wrapper resolves hostnames), so the address is built directly —
            // no string round-trip, and IPv6 needs no bracket handling.
            let resolved = std::net::SocketAddr::new(*ip, *port_num);

            let domain = match resolved {
                std::net::SocketAddr::V4(_) => libc::AF_INET,
                std::net::SocketAddr::V6(_) => libc::AF_INET6,
            };

            let fd = unsafe { libc::socket(domain, libc::SOCK_STREAM | libc::SOCK_NONBLOCK, 0) };
            if fd < 0 {
                return Err(crate::io::os_error("connect: socket() failed"));
            }

            // SAFETY: `fd` is a fresh socket we own. Holding it as an
            // OwnedFd means every early return below closes it for us.
            let sock = unsafe { OwnedFd::from_raw_fd(fd) };
            let (sa_bytes, sa_len) = crate::io::sockaddr::build_inet(&resolved);
            (sock, sa_bytes, sa_len)
        }
        ConnectAddr::Unix { path, .. } => {
            let fd =
                unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_NONBLOCK, 0) };
            if fd < 0 {
                return Err(crate::io::os_error("connect: socket() failed"));
            }
            // SAFETY: see the TCP arm — `sock` owns `fd` from here on.
            let sock = unsafe { OwnedFd::from_raw_fd(fd) };
            let (sun, addr_len) = match crate::io::sockaddr::build_unix(path) {
                Ok(result) => result,
                // `sock` drops here, closing the socket.
                Err(msg) => return Err(format!("connect: {}", msg)),
            };
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    &sun as *const _ as *const u8,
                    std::mem::size_of::<libc::sockaddr_un>(),
                )
                .to_vec()
            };
            (sock, bytes, addr_len)
        }
    };

    // Apply socket options before connect
    apply_socket_options(sock_fd.as_raw_fd(), addr.options());

    // Stash the sockaddr in the caller's buffer so it lives until the CQE
    // completes. The caller passes its buf_handle — no second allocation.
    let buf = buffer_pool.get_mut(buf_handle);
    buf.extend_from_slice(&sockaddr_buf);

    let connect_sqe = opcode::Connect::new(
        Fd(sock_fd.as_raw_fd()),
        buf.as_ptr() as *const libc::sockaddr,
        sockaddr_len,
    )
    .build()
    .user_data(id.as_u64());

    let connect_sqe = if timeout.is_some() {
        connect_sqe.flags(io_uring::squeue::Flags::IO_LINK)
    } else {
        connect_sqe
    };

    unsafe {
        // On push failure, `sock_fd` (still an OwnedFd) drops and closes.
        ring.submission()
            .push(&connect_sqe)
            .map_err(|e| format!("io/submit: io_uring submission queue full: {}", e))?;
    }

    // The connect SQE now references the socket; hand the kernel an
    // un-owned raw fd. The caller stashes it in PendingOp::Connect and the
    // completion path takes ownership (or closes it on failure). A later
    // failure here leaks the fd, matching the pre-RAII behaviour.
    let sock_fd = sock_fd.into_raw_fd();

    if let Some(dur) = timeout {
        let ts = io_uring::types::Timespec::new()
            .sec(dur.as_secs())
            .nsec(dur.subsec_nanos());
        let timeout_sqe = opcode::LinkTimeout::new(&ts)
            .build()
            .user_data(id.as_u64() | TIMEOUT_USER_DATA_TAG);
        unsafe {
            ring.submission()
                .push(&timeout_sqe)
                .map_err(|_| "io/submit: io_uring submission queue full".to_string())?;
        }
    }

    ring.submit()
        .map_err(|e| format!("io/submit: io_uring submit failed: {}", e))?;
    Ok(sock_fd)
}
pub(crate) fn submit_uring_sendto(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    fd: RawFd,
    payload: &[u8],
    timeout: Option<Duration>,
    buffer_pool: &mut BufferPool,
) -> Result<(), String> {
    use io_uring::opcode;
    use io_uring::types::Fd;

    // Parse address from payload (format: "addr:port\0payload")
    let nul_pos = payload
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(payload.len());
    let addr_str = String::from_utf8_lossy(&payload[..nul_pos]).to_string();
    let data = if nul_pos < payload.len() {
        &payload[nul_pos + 1..]
    } else {
        &[]
    };

    // Parse address
    match addr_str.parse::<std::net::SocketAddr>() {
        Ok(dest) => {
            let (sockaddr_bytes, sockaddr_len) = crate::io::sockaddr::build_inet(&dest);

            // Pack sockaddr + payload into one buffer so both survive until
            // the CQE completes.  sockaddr at offset 0, payload after it.
            let buf_handle = buffer_pool.alloc(0);
            let buf = buffer_pool.get_mut(buf_handle);
            buf.extend_from_slice(&sockaddr_bytes);
            buf.extend_from_slice(data);

            let sockaddr_ptr = buf.as_ptr() as *const libc::sockaddr;
            let payload_ptr = unsafe { buf.as_ptr().add(sockaddr_bytes.len()) };
            let sendto_sqe = opcode::Send::new(Fd(fd), payload_ptr, data.len() as u32)
                .dest_addr(sockaddr_ptr)
                .dest_addr_len(sockaddr_len)
                .build()
                .user_data(id.as_u64());

            let sendto_sqe = if timeout.is_some() {
                sendto_sqe.flags(io_uring::squeue::Flags::IO_LINK)
            } else {
                sendto_sqe
            };

            unsafe {
                ring.submission()
                    .push(&sendto_sqe)
                    .map_err(|_| "io/submit: io_uring submission queue full".to_string())?;
            }

            if let Some(dur) = timeout {
                let ts = io_uring::types::Timespec::new()
                    .sec(dur.as_secs())
                    .nsec(dur.subsec_nanos());
                let timeout_sqe = opcode::LinkTimeout::new(&ts)
                    .build()
                    .user_data(id.as_u64() | TIMEOUT_USER_DATA_TAG);
                unsafe {
                    ring.submission()
                        .push(&timeout_sqe)
                        .map_err(|_| "io/submit: io_uring submission queue full".to_string())?;
                }
            }

            ring.submit()
                .map_err(|e| format!("io/submit: io_uring submit failed: {}", e))?;
            Ok(())
        }
        Err(_) => Err("invalid address format".to_string()),
    }
}
/// Scratch layout for RecvMsg control structures: `[msghdr | iovec |
/// sockaddr_storage]`.
///
/// The msghdr, iovec, and sockaddr_storage are packed into one buffer-pool
/// allocation so they stay pinned until the CQE completes. Unlike a plain recv,
/// the **payload does not live here** — the iovec points straight at the
/// `:data` LBytes buffer of the pre-allocated result struct (born on the
/// requesting fiber's heap), so the datagram is received zero-copy into the
/// value the fiber will receive. The fiber is parked, so the raw pointer into
/// its buffer is stable for the lifetime of the in-flight SQE.
pub(crate) fn submit_uring_recvfrom(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    fd: RawFd,
    count: usize,
    result: &crate::value::Value,
    timeout: Option<Duration>,
    buffer_pool: &mut BufferPool,
) -> Result<(), String> {
    use io_uring::opcode;
    use io_uring::types::Fd;

    let msghdr_size = std::mem::size_of::<libc::msghdr>();
    let iovec_size = std::mem::size_of::<libc::iovec>();
    let sockaddr_size = std::mem::size_of::<libc::sockaddr_storage>();
    // Scratch holds only the control structs; the payload goes into `:data`.
    let total = msghdr_size + iovec_size + sockaddr_size;

    // Writeable pointer into the pre-allocated `:data` buffer (`count` bytes).
    let data_buf = crate::value::sorted_struct_get(
        result.as_struct().expect("recv result must be a struct"),
        &crate::value::heap::TableKey::Keyword("data".into()),
    )
    .copied()
    .expect("recv result must have :data");
    // SAFETY: the requesting fiber is parked; the kernel writes the datagram
    // through this pointer before the fiber is resumed.
    let (data_ptr, data_len) = unsafe { crate::io::request::writeable_buffer_ptr(&data_buf) };
    debug_assert_eq!(data_len, count);

    let buf_handle = buffer_pool.alloc(0);
    let buf = buffer_pool.get_mut(buf_handle);
    buf.resize(total, 0);

    let buf_ptr = buf.as_mut_ptr();

    unsafe {
        // iovec at offset msghdr_size — payload target is the fiber-heap buffer.
        let iov_ptr = buf_ptr.add(msghdr_size) as *mut libc::iovec;
        (*iov_ptr).iov_base = data_ptr as *mut _;
        (*iov_ptr).iov_len = count;

        // msghdr at offset 0
        let msg_ptr = buf_ptr as *mut libc::msghdr;
        (*msg_ptr).msg_name = buf_ptr.add(msghdr_size + iovec_size) as *mut _;
        (*msg_ptr).msg_namelen = sockaddr_size as libc::socklen_t;
        (*msg_ptr).msg_iov = iov_ptr;
        (*msg_ptr).msg_iovlen = 1;
        (*msg_ptr).msg_control = std::ptr::null_mut();
        (*msg_ptr).msg_controllen = 0;
        (*msg_ptr).msg_flags = 0;
    }

    let recvfrom_sqe = opcode::RecvMsg::new(Fd(fd), buf_ptr as *mut libc::msghdr)
        .build()
        .user_data(id.as_u64());

    let recvfrom_sqe = if timeout.is_some() {
        recvfrom_sqe.flags(io_uring::squeue::Flags::IO_LINK)
    } else {
        recvfrom_sqe
    };

    unsafe {
        ring.submission()
            .push(&recvfrom_sqe)
            .map_err(|_| "io/submit: io_uring submission queue full".to_string())?;
    }

    if let Some(dur) = timeout {
        let ts = io_uring::types::Timespec::new()
            .sec(dur.as_secs())
            .nsec(dur.subsec_nanos());
        let timeout_sqe = opcode::LinkTimeout::new(&ts)
            .build()
            .user_data(id.as_u64() | TIMEOUT_USER_DATA_TAG);
        unsafe {
            ring.submission()
                .push(&timeout_sqe)
                .map_err(|_| "io/submit: io_uring submission queue full".to_string())?;
        }
    }

    ring.submit()
        .map_err(|e| format!("io/submit: io_uring submit failed: {}", e))?;
    Ok(())
}
pub(crate) fn submit_uring_shutdown(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    fd: RawFd,
    how: i32,
    timeout: Option<Duration>,
    _buffer_pool: &mut BufferPool,
) -> Result<(), String> {
    use io_uring::opcode;
    use io_uring::types::Fd;

    let shutdown_sqe = opcode::Shutdown::new(Fd(fd), how)
        .build()
        .user_data(id.as_u64());

    let shutdown_sqe = if timeout.is_some() {
        shutdown_sqe.flags(io_uring::squeue::Flags::IO_LINK)
    } else {
        shutdown_sqe
    };

    unsafe {
        ring.submission()
            .push(&shutdown_sqe)
            .map_err(|_| "io/submit: io_uring submission queue full".to_string())?;
    }

    if let Some(dur) = timeout {
        let ts = io_uring::types::Timespec::new()
            .sec(dur.as_secs())
            .nsec(dur.subsec_nanos());
        let timeout_sqe = opcode::LinkTimeout::new(&ts)
            .build()
            .user_data(id.as_u64() | TIMEOUT_USER_DATA_TAG);
        unsafe {
            ring.submission()
                .push(&timeout_sqe)
                .map_err(|_| "io/submit: io_uring submission queue full".to_string())?;
        }
    }

    ring.submit()
        .map_err(|e| format!("io/submit: io_uring submit failed: {}", e))?;
    Ok(())
}
