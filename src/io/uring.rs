//! io_uring submission and wait methods for async I/O.

use crate::io::aio::{Completion, PendingOp, TIMEOUT_USER_DATA_TAG};
use crate::io::completion::process_raw_completion;
use crate::io::pool::{BufferHandle, BufferPool};
use crate::io::request::ConnectAddr;
use crate::io::types::{FdState, PortKey};
use std::collections::{HashMap, VecDeque};
use std::os::unix::io::RawFd;
use std::time::Duration;

#[allow(clippy::too_many_arguments)]
pub(super) fn submit_uring(
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

pub(super) fn submit_uring_connect(
    ring: &mut io_uring::IoUring,
    id: u64,
    addr: &ConnectAddr,
    timeout: Option<Duration>,
    _buffer_pool: &mut BufferPool,
) -> Result<(), String> {
    // For io_uring connect, we need to resolve the address and create a socket
    // This is complex, so we'll keep the thread-pool fallback for now
    // and just return an error to fall back to thread pool
    let _ = (ring, id, addr, timeout);
    Err("io_uring connect not yet implemented; using thread pool".to_string())
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
            let (sockaddr, sockaddr_len) = match dest {
                std::net::SocketAddr::V4(v4) => {
                    let mut sin: libc::sockaddr_in = unsafe { std::mem::zeroed() };
                    sin.sin_family = libc::AF_INET as libc::sa_family_t;
                    sin.sin_port = v4.port().to_be();
                    sin.sin_addr.s_addr = u32::from(*v4.ip()).to_be();
                    let ptr = &sin as *const libc::sockaddr_in as *const libc::sockaddr;
                    let len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
                    (ptr, len)
                }
                std::net::SocketAddr::V6(v6) => {
                    let mut sin6: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
                    sin6.sin6_family = libc::AF_INET6 as libc::sa_family_t;
                    sin6.sin6_port = v6.port().to_be();
                    sin6.sin6_addr.s6_addr = v6.ip().octets();
                    let ptr = &sin6 as *const libc::sockaddr_in6 as *const libc::sockaddr;
                    let len = std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t;
                    (ptr, len)
                }
            };

            let buf_handle = buffer_pool.alloc(data.len());
            let buf = buffer_pool.get_mut(buf_handle);
            buf.extend_from_slice(data);

            let sendto_sqe = opcode::Send::new(Fd(fd), buf.as_ptr(), buf.len() as u32)
                .dest_addr(sockaddr)
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

    let buf_handle = buffer_pool.alloc(count + std::mem::size_of::<libc::sockaddr_storage>() + 4);
    let buf = buffer_pool.get_mut(buf_handle);
    buf.resize(count, 0);

    let recvfrom_sqe = opcode::Recv::new(Fd(fd), buf.as_mut_ptr(), buf.len() as u32)
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

pub(super) fn wait_uring(
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
        let user_data = cqe.user_data();
        let result_code = cqe.result();

        // Skip timeout CQEs (they have the high bit set)
        if user_data & TIMEOUT_USER_DATA_TAG != 0 {
            continue;
        }

        let id = user_data;
        if let Some(pending_op) = pending.remove(&id) {
            let data = if result_code > 0 {
                let buf = buffer_pool.get_mut(pending_op.buffer_handle);
                buf[..result_code as usize].to_vec()
            } else {
                Vec::new()
            };
            let completion = process_raw_completion(
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
