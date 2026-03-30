//! io_uring submission and wait methods for async I/O.

use crate::io::aio::TIMEOUT_USER_DATA_TAG;
use crate::io::completion::process_raw_completion;
use crate::io::pending::PendingOp;
use crate::io::pool::{BufferHandle, BufferPool};
use crate::io::request::{ConnectAddr, IoOp};
use crate::io::types::{FdState, PortKey};
use crate::io::Completion;
use crate::port::{Port, PortKind};
use std::collections::{HashMap, VecDeque};
use std::os::unix::io::RawFd;
use std::time::Duration;

/// Submit a stream I/O operation (Read, ReadLine, ReadAll, Write, Flush).
///
/// `read_buffered`: for Read ops, the number of bytes already sitting in the
/// fd_state buffer. The kernel read is reduced by this amount so the
/// completion handler can prepend the buffered prefix.
#[allow(clippy::too_many_arguments)]
pub(super) fn submit_uring_stream(
    ring: &mut io_uring::IoUring,
    id: u64,
    fd: RawFd,
    op: &IoOp,
    timeout: Option<Duration>,
    buffer_pool: &mut BufferPool,
    buf_handle: BufferHandle,
    read_buffered: usize,
) -> Result<(), String> {
    use io_uring::opcode;
    use io_uring::types::Fd;

    let entry = match op {
        IoOp::ReadLine | IoOp::ReadAll => {
            let buf = buffer_pool.get_mut(buf_handle);
            buf.resize(4096, 0);
            opcode::Read::new(Fd(fd), buf.as_mut_ptr(), buf.len() as u32)
                .offset(u64::MAX)
                .build()
                .user_data(id)
        }
        IoOp::Read { count } => {
            let buf = buffer_pool.get_mut(buf_handle);
            buf.resize(*count - read_buffered, 0);
            opcode::Read::new(Fd(fd), buf.as_mut_ptr(), buf.len() as u32)
                .offset(u64::MAX)
                .build()
                .user_data(id)
        }
        IoOp::Write { data } => {
            let bytes = crate::io::aio::AsyncBackend::extract_write_bytes(data);
            let buf = buffer_pool.get_mut(buf_handle);
            buf.clear();
            buf.extend_from_slice(&bytes);
            opcode::Write::new(Fd(fd), buf.as_ptr(), buf.len() as u32)
                .offset(u64::MAX)
                .build()
                .user_data(id)
        }
        IoOp::Flush => opcode::Fsync::new(Fd(fd)).build().user_data(id),
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
            .user_data(id | TIMEOUT_USER_DATA_TAG);
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

pub(super) fn submit_uring_accept(
    ring: &mut io_uring::IoUring,
    id: u64,
    fd: RawFd,
    timeout: Option<Duration>,
) -> Result<(), String> {
    use io_uring::opcode;
    use io_uring::types::Fd;

    let accept_sqe = opcode::Accept::new(Fd(fd), std::ptr::null_mut(), std::ptr::null_mut())
        .build()
        .user_data(id);

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
            .user_data(id | TIMEOUT_USER_DATA_TAG);
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
pub(super) fn submit_uring_connect(
    ring: &mut io_uring::IoUring,
    id: u64,
    addr: &ConnectAddr,
    timeout: Option<Duration>,
    buffer_pool: &mut BufferPool,
    buf_handle: BufferHandle,
) -> Result<RawFd, String> {
    use io_uring::opcode;
    use io_uring::types::Fd;

    let (sock_fd, sockaddr_buf, sockaddr_len) = match addr {
        ConnectAddr::Tcp {
            addr: host,
            port: port_num,
        } => {
            let resolved = format!("{}:{}", host, port_num)
                .parse::<std::net::SocketAddr>()
                .map_err(|e| format!("connect: invalid address: {}", e))?;

            let domain = match resolved {
                std::net::SocketAddr::V4(_) => libc::AF_INET,
                std::net::SocketAddr::V6(_) => libc::AF_INET6,
            };

            let fd = unsafe { libc::socket(domain, libc::SOCK_STREAM | libc::SOCK_NONBLOCK, 0) };
            if fd < 0 {
                return Err(format!(
                    "connect: socket() failed: {}",
                    std::io::Error::last_os_error()
                ));
            }

            let (sa_bytes, sa_len) = crate::io::sockaddr::build_inet(&resolved);
            (fd, sa_bytes, sa_len)
        }
        ConnectAddr::Unix { path } => {
            let fd =
                unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_NONBLOCK, 0) };
            if fd < 0 {
                return Err(format!(
                    "connect: socket() failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let (sun, addr_len) = match crate::io::sockaddr::build_unix(path) {
                Ok(result) => result,
                Err(msg) => {
                    unsafe { libc::close(fd) };
                    return Err(format!("connect: {}", msg));
                }
            };
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    &sun as *const _ as *const u8,
                    std::mem::size_of::<libc::sockaddr_un>(),
                )
                .to_vec()
            };
            (fd, bytes, addr_len)
        }
    };

    // Stash the sockaddr in the caller's buffer so it lives until the CQE
    // completes. The caller passes its buf_handle — no second allocation.
    let buf = buffer_pool.get_mut(buf_handle);
    buf.extend_from_slice(&sockaddr_buf);

    let connect_sqe = opcode::Connect::new(
        Fd(sock_fd),
        buf.as_ptr() as *const libc::sockaddr,
        sockaddr_len,
    )
    .build()
    .user_data(id);

    let connect_sqe = if timeout.is_some() {
        connect_sqe.flags(io_uring::squeue::Flags::IO_LINK)
    } else {
        connect_sqe
    };

    unsafe {
        ring.submission().push(&connect_sqe).map_err(|e| {
            libc::close(sock_fd);
            format!("io/submit: io_uring submission queue full: {}", e)
        })?;
    }

    if let Some(dur) = timeout {
        let ts = io_uring::types::Timespec::new()
            .sec(dur.as_secs())
            .nsec(dur.subsec_nanos());
        let timeout_sqe = opcode::LinkTimeout::new(&ts)
            .build()
            .user_data(id | TIMEOUT_USER_DATA_TAG);
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

pub(super) fn submit_uring_sendto(
    ring: &mut io_uring::IoUring,
    id: u64,
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
                .user_data(id);

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
                    .user_data(id | TIMEOUT_USER_DATA_TAG);
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

/// Buffer layout for RecvMsg: `[msghdr | iovec | sockaddr_storage | data(count)]`
///
/// The msghdr, iovec, sockaddr_storage, and data buffer are all packed into
/// one contiguous allocation so they stay pinned until the CQE completes.
pub(super) fn submit_uring_recvfrom(
    ring: &mut io_uring::IoUring,
    id: u64,
    fd: RawFd,
    count: usize,
    timeout: Option<Duration>,
    buffer_pool: &mut BufferPool,
) -> Result<(), String> {
    use io_uring::opcode;
    use io_uring::types::Fd;

    let msghdr_size = std::mem::size_of::<libc::msghdr>();
    let iovec_size = std::mem::size_of::<libc::iovec>();
    let sockaddr_size = std::mem::size_of::<libc::sockaddr_storage>();
    let total = msghdr_size + iovec_size + sockaddr_size + count;

    let buf_handle = buffer_pool.alloc(0);
    let buf = buffer_pool.get_mut(buf_handle);
    buf.resize(total, 0);

    let buf_ptr = buf.as_mut_ptr();

    unsafe {
        // iovec at offset msghdr_size
        let iov_ptr = buf_ptr.add(msghdr_size) as *mut libc::iovec;
        (*iov_ptr).iov_base = buf_ptr.add(msghdr_size + iovec_size + sockaddr_size) as *mut _;
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
        .user_data(id);

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
            .user_data(id | TIMEOUT_USER_DATA_TAG);
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

pub(super) fn submit_uring_shutdown(
    ring: &mut io_uring::IoUring,
    id: u64,
    fd: RawFd,
    how: i32,
    timeout: Option<Duration>,
    _buffer_pool: &mut BufferPool,
) -> Result<(), String> {
    use io_uring::opcode;
    use io_uring::types::Fd;

    let shutdown_sqe = opcode::Shutdown::new(Fd(fd), how).build().user_data(id);

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
            .user_data(id | TIMEOUT_USER_DATA_TAG);
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

/// Submit a standalone Timeout SQE for ev/sleep.
///
/// Unlike LinkTimeout (which cancels a linked op), this is a freestanding
/// timer. The CQE fires after the duration with result_code = -ETIME (62).
/// We treat -ETIME as success for sleep (the timer expired normally).
pub(super) fn submit_uring_sleep(
    ring: &mut io_uring::IoUring,
    id: u64,
    duration: Duration,
) -> Result<(), String> {
    use io_uring::opcode;

    let ts = io_uring::types::Timespec::new()
        .sec(duration.as_secs())
        .nsec(duration.subsec_nanos());
    let timeout_sqe = opcode::Timeout::new(&ts).build().user_data(id);
    unsafe {
        ring.submission()
            .push(&timeout_sqe)
            .map_err(|_| "io/submit: io_uring submission queue full".to_string())?;
    }
    ring.submit()
        .map_err(|e| format!("io/submit: io_uring submit failed: {}", e))?;
    Ok(())
}

/// Submit IORING_OP_WAITID to wait for a subprocess to exit.
///
/// The kernel fills `infop` when the child exits. The `siginfo_t` must
/// remain valid until the CQE arrives — the caller stores it in PendingOp.
///
/// Requires Linux kernel 6.7+. If the opcode is unsupported, the CQE
/// returns result = -EINVAL (22).
///
/// # Safety
/// `siginfo_ptr` must point to a valid, heap-allocated `siginfo_t`
/// that outlives the submitted SQE. The caller (submit_process_wait) allocates
/// via `Box::into_raw` and frees via completion processing or error path.
pub(super) fn submit_uring_process_wait(
    ring: &mut io_uring::IoUring,
    id: u64,
    pid: u32,
    siginfo_ptr: *mut libc::siginfo_t,
) -> Result<(), String> {
    use io_uring::opcode;

    let entry = opcode::WaitId::new(libc::P_PID, pid as libc::id_t, libc::WEXITED)
        .infop(siginfo_ptr as *const libc::siginfo_t)
        .build()
        .user_data(id);

    // SAFETY: `entry` references `siginfo_ptr` which is kept alive by the
    // caller for the lifetime of the pending op. The SQE is submitted
    // immediately here, and the kernel will fill siginfo on child exit.
    unsafe {
        ring.submission()
            .push(&entry)
            .map_err(|_| "io/submit: io_uring submission queue full".to_string())?;
    }
    ring.submit()
        .map_err(|e| format!("io/submit: io_uring submit failed: {}", e))?;
    Ok(())
}

/// Submit IORING_OP_OPENAT via io_uring.
///
/// The null-terminated path is stored in the buffer pool slot so it stays
/// pinned until the CQE completes. Caller passes `buf_handle` which is already
/// allocated (with 0 bytes). The path bytes are extended into it here.
///
/// On success, the CQE result is the new file descriptor (>= 0).
/// On failure, the CQE result is -errno.
/// On timeout (linked timeout fires first), result is -ECANCELED (errno 125).
#[allow(clippy::too_many_arguments)]
pub(super) fn submit_uring_open(
    ring: &mut io_uring::IoUring,
    id: u64,
    path: &std::ffi::CStr,
    flags: i32,
    mode: u32,
    timeout: Option<Duration>,
    buffer_pool: &mut BufferPool,
    buf_handle: BufferHandle,
) -> Result<(), String> {
    use io_uring::opcode;
    use io_uring::types::Fd;

    // Store the null-terminated path bytes in the buffer pool slot.
    // The path must remain valid until ring.submit() returns: the kernel reads the
    // pathname pointer during the io_uring_enter(2) syscall and copies it into kernel
    // memory before returning (kernels >= 5.5; we require a modern kernel for io_uring).
    // We keep the buffer allocated until the CQE arrives (via drain_cqes releasing
    // buf_handle) as a conservative strategy, consistent with submit_uring_connect
    // stashing sockaddr bytes.
    //
    // Safety invariant: path_ptr is valid from this point until ring.submit() returns.
    // The buffer pool Vec<u8> is not dropped or reallocated between buf.as_ptr() capture
    // and ring.submit() because: (a) no other buffer_pool mutation occurs in this
    // function after buf.as_ptr(); (b) Vec<u8> heap data is stable even if the outer
    // pool Vec<Option<Vec<u8>>> reallocates on subsequent alloc() calls.
    let buf = buffer_pool.get_mut(buf_handle);
    buf.extend_from_slice(path.to_bytes_with_nul());
    let path_ptr = buf.as_ptr() as *const libc::c_char;

    let open_sqe = opcode::OpenAt::new(Fd(libc::AT_FDCWD), path_ptr)
        .flags(flags)
        .mode(mode)
        .build()
        .user_data(id);

    let open_sqe = if timeout.is_some() {
        open_sqe.flags(io_uring::squeue::Flags::IO_LINK)
    } else {
        open_sqe
    };

    // SAFETY: See invariant above — path_ptr is valid through ring.submit().
    unsafe {
        ring.submission()
            .push(&open_sqe)
            .map_err(|_| "io/submit: io_uring submission queue full".to_string())?;
    }

    if let Some(dur) = timeout {
        let ts = io_uring::types::Timespec::new()
            .sec(dur.as_secs())
            .nsec(dur.subsec_nanos());
        let timeout_sqe = opcode::LinkTimeout::new(&ts)
            .build()
            .user_data(id | TIMEOUT_USER_DATA_TAG);
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

/// Submit an AsyncCancel SQE to cancel a pending operation.
///
/// The cancelled operation will generate a CQE with result = -ECANCELED.
/// The cancel SQE itself generates a CQE with the high-bit tagged user_data
/// (same as timeout CQEs), so drain_cqes skips it.
pub(super) fn submit_uring_cancel(
    ring: &mut io_uring::IoUring,
    target_user_data: u64,
) -> Result<(), String> {
    use io_uring::opcode;

    let cancel_sqe = opcode::AsyncCancel::new(target_user_data)
        .build()
        .user_data(target_user_data | TIMEOUT_USER_DATA_TAG);
    unsafe {
        ring.submission()
            .push(&cancel_sqe)
            .map_err(|_| "io/cancel: io_uring submission queue full".to_string())?;
    }
    ring.submit()
        .map_err(|e| format!("io/cancel: io_uring submit failed: {}", e))?;
    Ok(())
}

/// Drain all available CQEs from the completion ring.
///
/// This is the **single** CQE processing path — used by both poll (non-blocking)
/// and wait (after blocking). Handles:
/// - Timeout CQE filtering (high-bit user_data tag)
/// - Connect fd cleanup on error
/// - IoOp-aware buffer extraction (only reads buffer for stream reads)
pub(super) fn drain_cqes(
    ring: &mut io_uring::IoUring,
    pending: &mut HashMap<u64, PendingOp>,
    buffer_pool: &mut BufferPool,
    fd_states: &mut HashMap<PortKey, FdState>,
    completions: &mut VecDeque<Completion>,
) {
    // Collect ReadLine and short-Read ops that need re-submission (can't
    // submit SQEs while iterating the CQ ring).
    let mut read_resubmits: Vec<(u64, RawFd, usize, PendingOp)> = Vec::new();

    for cqe in ring.completion() {
        let user_data = cqe.user_data();
        let result_code = cqe.result();

        // Timeout CQEs have the high bit set — skip them.
        if user_data & TIMEOUT_USER_DATA_TAG != 0 {
            continue;
        }

        let id = user_data;
        if let Some(mut pending_op) = pending.remove(&id) {
            // Connect: on failure, close the pre-created socket.
            if let PendingOp::Connect {
                ref mut connect_fd, ..
            } = pending_op
            {
                if let Some(fd) = *connect_fd {
                    if result_code < 0 {
                        unsafe { libc::close(fd) };
                        *connect_fd = None;
                    }
                }
            }

            // Only read from the buffer for stream I/O ops where result_code
            // is a byte count. Accept (result = new fd), Connect (result = 0),
            // Sleep, Shutdown, Flush — no buffer data.
            //
            // RecvFrom uses RecvMsg with a packed buffer layout:
            //   [msghdr | iovec | sockaddr_storage | data]
            // Extract the sockaddr and payload, encode as:
            //   addr_len(4 LE) + sockaddr_storage + payload
            // to match the thread pool format expected by completion.rs.
            let buf_handle = pending_op.buffer_handle();
            let data = match &pending_op {
                PendingOp::Port { op, .. } => match op {
                    IoOp::RecvFrom { .. } if result_code > 0 => {
                        let msghdr_size = std::mem::size_of::<libc::msghdr>();
                        let iovec_size = std::mem::size_of::<libc::iovec>();
                        let sockaddr_size = std::mem::size_of::<libc::sockaddr_storage>();
                        let buf = buffer_pool.get_mut(buf_handle);

                        // Read actual address length from msghdr (kernel updates msg_namelen)
                        let addr_len = unsafe {
                            let msg_ptr = buf.as_ptr() as *const libc::msghdr;
                            (*msg_ptr).msg_namelen
                        };

                        // Extract sockaddr_storage bytes
                        let sa_start = msghdr_size + iovec_size;
                        let sa_bytes = buf[sa_start..sa_start + sockaddr_size].to_vec();

                        // Extract payload bytes (result_code = bytes received)
                        let data_start = msghdr_size + iovec_size + sockaddr_size;
                        let payload = buf[data_start..data_start + result_code as usize].to_vec();

                        // Encode in thread pool format: addr_len(4 LE) + sockaddr_storage + payload
                        let mut encoded = Vec::with_capacity(4 + sockaddr_size + payload.len());
                        encoded.extend_from_slice(&addr_len.to_le_bytes());
                        encoded.extend_from_slice(&sa_bytes);
                        encoded.extend_from_slice(&payload);
                        encoded
                    }
                    IoOp::ReadLine | IoOp::Read { .. } | IoOp::ReadAll if result_code > 0 => {
                        let buf = buffer_pool.get_mut(buf_handle);
                        buf[..result_code as usize].to_vec()
                    }
                    _ => Vec::new(),
                },
                PendingOp::WatchNext { .. } if result_code > 0 => {
                    let buf = buffer_pool.get_mut(buf_handle);
                    buf[..result_code as usize].to_vec()
                }
                _ => Vec::new(),
            };

            // ReadLine re-submission: if the read returned data but no newline
            // was found (combined with any previously buffered bytes), buffer
            // the data and schedule another read instead of returning a
            // truncated line.
            if let PendingOp::Port {
                op: IoOp::ReadLine,
                ref port_key,
                ..
            } = pending_op
            {
                if result_code > 0 {
                    let state = fd_states
                        .entry(port_key.clone())
                        .or_insert_with(FdState::new);
                    let has_newline = state.buffer.iter().chain(data.iter()).any(|&b| b == b'\n');
                    if !has_newline {
                        state.buffer.extend_from_slice(&data);
                        buffer_pool.release(buf_handle);
                        let fd = match port_key {
                            PortKey::Fd(raw) => *raw,
                            PortKey::Stdout => 1,
                            PortKey::Stderr => 2,
                            PortKey::Stdin => unreachable!(),
                        };
                        // size=4096 for ReadLine (variable-length read)
                        read_resubmits.push((id, fd, 4096, pending_op));
                        continue;
                    }
                }
            }

            // Read short-read re-submission: regular files may return short
            // reads before EOF (rare but POSIX-legal). Buffer partial data
            // and resubmit for the remainder. Stream sockets (TCP, Unix)
            // are excluded — port/read returns "up to N bytes" per POSIX
            // semantics, so a short read is a normal completion.
            if let PendingOp::Port {
                op: IoOp::Read { count },
                ref port_key,
                ref port,
                ..
            } = pending_op
            {
                let is_stream = port
                    .as_external::<Port>()
                    .map(|p| matches!(p.kind(), PortKind::TcpStream | PortKind::UnixStream))
                    .unwrap_or(false);
                if !is_stream && result_code > 0 {
                    let got = result_code as usize;
                    let state = fd_states
                        .entry(port_key.clone())
                        .or_insert_with(FdState::new);
                    let total = state.buffer.len() + got;
                    if total < count {
                        // Short read — buffer and resubmit for remainder.
                        state.buffer.extend_from_slice(&data);
                        buffer_pool.release(buf_handle);
                        let fd = match port_key {
                            PortKey::Fd(raw) => *raw,
                            PortKey::Stdout => 1,
                            PortKey::Stderr => 2,
                            PortKey::Stdin => unreachable!(),
                        };
                        let remaining = count - total;
                        read_resubmits.push((id, fd, remaining, pending_op));
                        continue;
                    }
                }
            }

            // ReadAll re-submission: buffer data and resubmit until EOF
            // (result_code == 0). ReadAll reads until the write end closes.
            if let PendingOp::Port {
                op: IoOp::ReadAll,
                ref port_key,
                ..
            } = pending_op
            {
                if result_code > 0 {
                    let state = fd_states
                        .entry(port_key.clone())
                        .or_insert_with(FdState::new);
                    state.buffer.extend_from_slice(&data);
                    buffer_pool.release(buf_handle);
                    let fd = match port_key {
                        PortKey::Fd(raw) => *raw,
                        PortKey::Stdout => 1,
                        PortKey::Stderr => 2,
                        PortKey::Stdin => unreachable!(),
                    };
                    read_resubmits.push((id, fd, 4096, pending_op));
                    continue;
                }
            }

            let completion = process_raw_completion(
                id,
                result_code,
                data,
                &pending_op,
                fd_states,
                buffer_pool,
                buf_handle,
            );
            completions.push_back(completion);
        }
    }

    // Re-submit ReadLine and short-Read ops that need more data.
    for (id, fd, size, pending_op) in read_resubmits {
        let new_buf = buffer_pool.alloc(size);
        let buf = buffer_pool.get_mut(new_buf);
        buf.resize(size, 0);
        let sqe = io_uring::opcode::Read::new(
            io_uring::types::Fd(fd),
            buf.as_mut_ptr(),
            buf.len() as u32,
        )
        .offset(u64::MAX)
        .build()
        .user_data(id);
        // Re-insert pending op with new buffer handle.
        let mut reinserted: PendingOp = pending_op;
        *reinserted.buffer_handle_mut() = new_buf;
        pending.insert(id, reinserted);
        unsafe {
            let _ = ring.submission().push(&sqe);
        }
    }
    if !ring.submission().is_empty() {
        let _ = ring.submit();
    }
}

pub(super) fn wait_uring(
    ring: &mut io_uring::IoUring,
    timeout: Option<u64>,
    pending: &mut HashMap<u64, PendingOp>,
    buffer_pool: &mut BufferPool,
    fd_states: &mut HashMap<PortKey, FdState>,
    completions: &mut VecDeque<Completion>,
) -> Result<(), String> {
    // Block until at least one CQE is available (or timeout).
    match timeout {
        Some(0) => {} // poll only — no wait
        Some(ms) => {
            let ts = io_uring::types::Timespec::new()
                .sec(ms / 1000)
                .nsec(((ms % 1000) * 1_000_000) as u32);
            let args = io_uring::types::SubmitArgs::new().timespec(&ts);
            loop {
                match ring.submitter().submit_with_args(1, &args) {
                    Ok(_) => break,
                    Err(e) if e.raw_os_error() == Some(libc::EINTR) => {
                        // Interrupted by a signal (e.g. SIGCHLD from a subprocess
                        // in a concurrent test). Retry — the timeout is still active.
                        continue;
                    }
                    Err(e) if e.raw_os_error() == Some(libc::ETIME) => {
                        // Timeout expired with no completions — that's valid.
                        break;
                    }
                    Err(e) => {
                        return Err(format!("io/wait: io_uring wait failed: {}", e));
                    }
                }
            }
        }
        None => loop {
            match ring.submit_and_wait(1) {
                Ok(_) => break,
                Err(e) if e.raw_os_error() == Some(libc::EINTR) => continue,
                Err(e) => {
                    return Err(format!("io/wait: io_uring wait failed: {}", e));
                }
            }
        },
    }

    drain_cqes(ring, pending, buffer_pool, fd_states, completions);

    // If drain_cqes resubmitted ops (ReadAll/ReadLine) and produced no
    // completions, loop to wait for the resubmitted read's CQE. Only do
    // this for blocking waits (no timeout) — callers with timeouts
    // should return and retry via the outer event loop.
    if timeout.is_none() {
        while completions.is_empty() && !pending.is_empty() {
            loop {
                match ring.submit_and_wait(1) {
                    Ok(_) => break,
                    Err(e) if e.raw_os_error() == Some(libc::EINTR) => continue,
                    Err(e) => {
                        return Err(format!("io/wait: io_uring wait failed: {}", e));
                    }
                }
            }
            drain_cqes(ring, pending, buffer_pool, fd_states, completions);
        }
    }
    Ok(())
}

/// Submit a read on an inotify fd to wait for filesystem events.
pub(super) fn submit_uring_watch_next(
    ring: &mut io_uring::IoUring,
    id: u64,
    fd: RawFd,
    buffer_pool: &mut BufferPool,
    buf_handle: BufferHandle,
) -> Result<(), String> {
    use io_uring::opcode;
    use io_uring::types::Fd;

    let buf = buffer_pool.get_mut(buf_handle);
    buf.resize(4096, 0);
    let sqe = opcode::Read::new(Fd(fd), buf.as_mut_ptr(), buf.len() as u32)
        .build()
        .user_data(id);

    unsafe {
        ring.submission()
            .push(&sqe)
            .map_err(|_| "io/submit: io_uring submission queue full".to_string())?;
    }
    ring.submit()
        .map_err(|e| format!("io/submit: io_uring submit failed: {}", e))?;
    Ok(())
}
