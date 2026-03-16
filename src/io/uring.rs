//! io_uring submission and wait methods for async I/O.

use crate::io::aio::TIMEOUT_USER_DATA_TAG;
use crate::io::completion::process_raw_completion;
use crate::io::pending::PendingOp;
use crate::io::pool::{BufferHandle, BufferPool};
use crate::io::request::{ConnectAddr, IoOp};
use crate::io::types::{FdState, PortKey};
use crate::io::Completion;
use std::collections::{HashMap, VecDeque};
use std::os::unix::io::RawFd;
use std::time::Duration;

/// Submit a stream I/O operation (Read, ReadLine, ReadAll, Write, Flush).
pub(super) fn submit_uring_stream(
    ring: &mut io_uring::IoUring,
    id: u64,
    fd: RawFd,
    op: &IoOp,
    timeout: Option<Duration>,
    buffer_pool: &mut BufferPool,
    buf_handle: BufferHandle,
) -> Result<(), String> {
    use io_uring::opcode;
    use io_uring::types::Fd;

    let entry = match op {
        IoOp::ReadLine | IoOp::ReadAll => {
            let buf = buffer_pool.get_mut(buf_handle);
            buf.resize(4096, 0);
            opcode::Read::new(Fd(fd), buf.as_mut_ptr(), buf.len() as u32)
                .build()
                .user_data(id)
        }
        IoOp::Read { count } => {
            let buf = buffer_pool.get_mut(buf_handle);
            buf.resize(*count, 0);
            opcode::Read::new(Fd(fd), buf.as_mut_ptr(), buf.len() as u32)
                .build()
                .user_data(id)
        }
        IoOp::Write { data } => {
            let bytes = crate::io::aio::AsyncBackend::extract_write_bytes(data);
            let buf = buffer_pool.get_mut(buf_handle);
            buf.clear();
            buf.extend_from_slice(&bytes);
            opcode::Write::new(Fd(fd), buf.as_ptr(), buf.len() as u32)
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
                _ => Vec::new(),
            };
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
    Ok(())
}
