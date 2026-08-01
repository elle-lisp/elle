use super::*;

/// Submit a standalone Timeout SQE for ev/sleep.
///
/// Unlike LinkTimeout (which cancels a linked op), this is a freestanding
/// timer. The CQE fires after the duration with result_code = -ETIME (62).
/// We treat -ETIME as success for sleep (the timer expired normally).
pub(crate) fn submit_uring_sleep(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    duration: Duration,
) -> Result<(), String> {
    use io_uring::opcode;

    let ts = io_uring::types::Timespec::new()
        .sec(duration.as_secs())
        .nsec(duration.subsec_nanos());
    let timeout_sqe = opcode::Timeout::new(&ts).build().user_data(id.as_u64());
    unsafe { submit_linked(ring, id, timeout_sqe, None) }
}
/// Submit IORING_OP_POLL_ADD to wait for a raw fd to become ready.
///
/// The CQE result contains the revents mask (which events are ready).
/// Used by `ev/poll-fd` for waiting on display connections, eventfds, etc.,
/// and by `chan/wait-ready` to park on a chan-select wake fd.
///
/// `timeout` plumbs through as a linked LinkTimeout SQE (same pattern
/// as Accept/Connect): when the timeout fires first, the kernel
/// cancels the poll and the CQE returns `-ECANCELED` (errno 125),
/// which downstream completion handlers map to a timeout result.
pub(crate) fn submit_uring_poll_add(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    fd: std::os::unix::io::RawFd,
    events: u32,
    timeout: Option<Duration>,
) -> Result<(), String> {
    use io_uring::opcode;

    let poll_sqe = opcode::PollAdd::new(io_uring::types::Fd(fd), events)
        .build()
        .user_data(id.as_u64());
    unsafe { submit_linked(ring, id, poll_sqe, timeout) }
}
/// Arm the standing one-shot `POLL_ADD(eventfd, POLLIN)` that bridges hub
/// completions into the io_uring wait. Its CQE carries `EVENTFD_USER_DATA`
/// (not a `SubmissionId`), which `drain_cqes` recognises before the `pending`
/// lookup. `POLL_ADD` is one-shot, so the wait/poll path re-arms it after each
/// firing. Built without a linked timeout — the bridge poll never times out.
pub(crate) fn arm_eventfd_poll(ring: &mut io_uring::IoUring, eventfd: RawFd) -> Result<(), String> {
    submit_uring_poll_add(
        ring,
        SubmissionId::from_raw(EVENTFD_USER_DATA),
        eventfd,
        libc::POLLIN as u32,
        None,
    )
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
pub(crate) fn submit_uring_process_wait(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    pid: u32,
    siginfo_ptr: *mut libc::siginfo_t,
) -> Result<(), String> {
    use io_uring::opcode;

    let entry = opcode::WaitId::new(libc::P_PID, pid as libc::id_t, libc::WEXITED)
        .infop(siginfo_ptr as *const libc::siginfo_t)
        .build()
        .user_data(id.as_u64());

    // SAFETY: `entry` references `siginfo_ptr` which is kept alive by the
    // caller for the lifetime of the pending op. The SQE is submitted
    // immediately here, and the kernel will fill siginfo on child exit.
    unsafe { submit_linked(ring, id, entry, None) }
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
pub(crate) fn submit_uring_open(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
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
        .user_data(id.as_u64());

    unsafe { submit_linked(ring, id, open_sqe, timeout) }
}
/// Submit an AsyncCancel SQE to cancel a pending operation.
///
/// The cancelled operation will generate a CQE with result = -ECANCELED.
/// The cancel SQE itself generates a CQE with the high-bit tagged user_data
/// (same as timeout CQEs), so drain_cqes skips it.
pub(crate) fn submit_uring_cancel(
    ring: &mut io_uring::IoUring,
    target: SubmissionId,
) -> Result<(), String> {
    use io_uring::opcode;

    let cancel_sqe = opcode::AsyncCancel::new(target.as_u64())
        .build()
        .user_data(target.as_u64() | TIMEOUT_USER_DATA_TAG);
    unsafe {
        ring.submission()
            .push(&cancel_sqe)
            .map_err(|_| "io/cancel: io_uring submission queue full".to_string())?;
    }
    ring.submit()
        .map_err(|e| format!("io/cancel: io_uring submit failed: {}", e))?;
    Ok(())
}
/// Submit a read on a signalfd to wait for the next POSIX signal
/// delivery. Linux-only — io_uring is not available on macOS.
///
/// signalfd is a regular pollable kernel fd: io_uring's
/// `IORING_OP_READ` is internally driven by the kernel's poll
/// pipeline, so a CQE fires as soon as the kernel queues a
/// `signalfd_siginfo` record without ever parking an elle-side
/// thread. This is the production path used by `submit_sig_next`
/// (`src/io/aio.rs`) on the `PlatformBackend::Uring` arm.
///
/// `signalfd(2)` writes one fixed-size `signalfd_siginfo` (128 bytes
/// on every Linux ABI we target) per queued signal. We size the read
/// for eight entries — enough to batch a `kill -USR1` burst without
/// re-submitting, while keeping per-watcher buffer cost bounded. The
/// CQE returns the byte count, which `SignalReceiver::parse_events`
/// (`src/io/sigfd.rs`) carves back into `SigEvent`s.
///
/// `buf_handle` is the buffer pool slot already allocated by the
/// caller (so the buffer survives until the CQE arrives even if the
/// fiber is suspended). We resize it to the entry-aligned size here
/// rather than leaving sizing to the caller — the size is a property
/// of signalfd, not of submit_sig_next.
pub(crate) fn submit_uring_sig_next(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    fd: RawFd,
    buffer_pool: &mut BufferPool,
    buf_handle: BufferHandle,
) -> Result<(), String> {
    use io_uring::opcode;
    use io_uring::types::Fd;

    let entry_size = std::mem::size_of::<libc::signalfd_siginfo>();
    let buf = buffer_pool.get_mut(buf_handle);
    buf.resize(entry_size * 8, 0);
    let sqe = opcode::Read::new(Fd(fd), buf.as_mut_ptr(), buf.len() as u32)
        .build()
        .user_data(id.as_u64());

    unsafe { submit_linked(ring, id, sqe, None) }
}
/// Submit a read on an inotify fd to wait for filesystem events.
pub(crate) fn submit_uring_watch_next(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
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
        .user_data(id.as_u64());

    unsafe { submit_linked(ring, id, sqe, None) }
}
