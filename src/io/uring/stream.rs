//! audited: 2026-09-23
//! Building the SQEs a port operation submits: the byte-stream reads and
//! writes, and the socket operations with builders of their own.
//!
//! src/io/AGENTS.md
//! docs/impl/io-bytes.md

use super::*;
use crate::io::landing::{copy_payload_into, payload_in_region, READ_ALL_CHUNK};
use crate::value::Value;

/// An SQE that reads at most `len` bytes into the caller's `buffer`, `at`
/// bytes in. The first submission and every resubmission build it the same
/// way, so the kernel always writes into the caller's region.
pub(super) fn read_into(
    id: SubmissionId,
    fd: RawFd,
    buffer: &Value,
    at: usize,
    len: usize,
) -> io_uring::squeue::Entry {
    // SAFETY: the buffer is the parked fiber's pre-allocated `LBytes`, held by
    // the pending entry until the completion arrives, and `at + len` stays
    // inside it.
    let (dst, cap) = unsafe { crate::io::request::writeable_buffer_ptr(buffer) };
    debug_assert!(at + len <= cap, "a read SQE ran past its buffer");
    io_uring::opcode::Read::new(io_uring::types::Fd(fd), unsafe { dst.add(at) }, len as u32)
        .offset(u64::MAX)
        .build()
        .user_data(id.as_u64())
}

/// An SQE that reads into the room past `acc`'s length, after making sure there
/// is a chunk of it. A `read-all` accumulates this way, so the kernel writes
/// straight into the accumulation.
pub(super) fn read_all_into(
    id: SubmissionId,
    fd: RawFd,
    acc: &mut Vec<u8>,
) -> io_uring::squeue::Entry {
    if acc.capacity() - acc.len() < READ_ALL_CHUNK {
        acc.reserve(READ_ALL_CHUNK);
    }
    let room = (acc.capacity() - acc.len()).min(MAX_SQE_BYTES);
    // SAFETY: the room past `len` is allocated, and the accumulation is neither
    // read nor grown until this SQE's completion arrives.
    let dst = unsafe { acc.as_mut_ptr().add(acc.len()) };
    io_uring::opcode::Read::new(io_uring::types::Fd(fd), dst, room as u32)
        .offset(u64::MAX)
        .build()
        .user_data(id.as_u64())
}

/// An SQE that writes as much of `bytes` as one SQE carries.
pub(super) fn write_from(id: SubmissionId, fd: RawFd, bytes: &[u8]) -> io_uring::squeue::Entry {
    let len = bytes.len().min(MAX_SQE_BYTES);
    io_uring::opcode::Write::new(io_uring::types::Fd(fd), bytes.as_ptr(), len as u32)
        .offset(u64::MAX)
        .build()
        .user_data(id.as_u64())
}

/// The bytes a write's SQEs read: its pooled copy when it has one, the
/// payload's own region bytes when it was handed over by address.
pub(super) fn write_source<'a>(
    payload: &'a Value,
    buffer_handle: Option<BufferHandle>,
    buffer_pool: &'a mut BufferPool,
) -> &'a [u8] {
    match buffer_handle {
        Some(bh) => buffer_pool.get_mut(bh),
        None => payload_in_region(payload)
            .expect("a write with no pooled copy is handed the kernel by address"),
    }
}

/// Submit one of the three reads into the caller's buffer, behind the `start`
/// bytes a borrowed remainder already fills there.
///
/// A line reads a page at a time, so the bytes past its newline that go back
/// to the port stay few. The byte- and cluster-counted reads read as much of
/// the buffer as one chunk holds. `drain_cqes` reads on into the tail.
pub(crate) fn submit_uring_read(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    fd: RawFd,
    op: &PortOp,
    start: usize,
    timeout: Option<Duration>,
) -> Result<(), String> {
    let (buffer, most) = match op {
        PortOp::ReadLine { buffer } => (buffer, 4096),
        PortOp::Read { buffer, .. } | PortOp::ReadExact { buffer, .. } => (buffer, MAX_READ_CHUNK),
        other => return Err(format!("io/submit: {other:?} does not read into a buffer")),
    };
    let cap = buffer.as_bytes().map_or(0, <[u8]>::len);
    let entry = read_into(id, fd, buffer, start, cap.saturating_sub(start).min(most));
    unsafe { submit_linked(ring, id, entry, timeout) }
}

/// Submit a byte-stream operation that is not one of the three buffered reads:
/// a `read-all`, a write, or a flush. The reads go through
/// [`submit_uring_read`].
///
/// A `read-all` reads into its pooled accumulation. A write with a pooled
/// buffer copies its payload there first and writes from the copy; a write
/// without one hands the kernel the payload's region bytes, which the pending
/// entry holds until the completion arrives (docs/impl/io-bytes.md).
pub(crate) fn submit_uring_stream(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    fd: RawFd,
    op: &PortOp,
    timeout: Option<Duration>,
    buffer_pool: &mut BufferPool,
    buf_handle: Option<BufferHandle>,
) -> Result<(), String> {
    let entry = match op {
        PortOp::ReadLine { .. } | PortOp::Read { .. } | PortOp::ReadExact { .. } => {
            return submit_uring_read(ring, id, fd, op, 0, timeout);
        }
        PortOp::ReadAll => {
            let bh = buf_handle.expect("ReadAll requires BufferHandle");
            read_all_into(id, fd, buffer_pool.get_mut(bh))
        }
        PortOp::Write { data } => {
            if let Some(bh) = buf_handle {
                copy_payload_into(data, buffer_pool.get_mut(bh));
            }
            // The whole payload stays where it is: the fd accepts only what
            // fits in its send buffer, and `drain_cqes` resubmits the tail
            // from the same source until every byte is gone.
            write_from(id, fd, write_source(data, buf_handle, buffer_pool))
        }
        PortOp::Flush => io_uring::opcode::Fsync::new(io_uring::types::Fd(fd))
            .build()
            .user_data(id.as_u64()),
        // The socket ops carry their own SQE builders (`submit_uring_accept`
        // and friends); `AsyncBackend::submit` routes them there instead.
        PortOp::Accept { .. }
        | PortOp::SendTo { .. }
        | PortOp::RecvFrom { .. }
        | PortOp::Shutdown { .. } => return Err(format!("io/submit: {:?} is not a stream op", op)),
    };

    unsafe { submit_linked(ring, id, entry, timeout) }
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

    unsafe { submit_linked(ring, id, accept_sqe, timeout) }
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
    // completion path takes ownership (or closes it on failure). A failure
    // between here and the submit leaks the descriptor.
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

            unsafe { submit_linked(ring, id, sendto_sqe, timeout) }
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
        &crate::value::heap::TableKey::keyword("data"),
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

    unsafe { submit_linked(ring, id, recvfrom_sqe, timeout) }
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

    unsafe { submit_linked(ring, id, shutdown_sqe, timeout) }
}
