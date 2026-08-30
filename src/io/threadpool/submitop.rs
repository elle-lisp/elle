use super::*;

impl CompletionHub {
    /// Submit a blocking I/O operation on a background worker thread; the worker
    /// reports its result back through the hub channel as a `RawCompletion::Pool`.
    ///
    /// `bounds` is how long the operation may wait and how `io/cancel` ends it.
    /// Every submission states one, so an operation that can wait for something
    /// that may never happen cannot be submitted without saying so — see
    /// `src/io/AGENTS.md` § "The stop pipe" for the two conditions that decide
    /// which kind an operation needs.
    ///
    /// How many operations may run at once is the OS's to say. The worker is
    /// started with `Builder::spawn` rather than `thread::spawn` for exactly
    /// that reason: `thread::spawn` panics when the OS refuses a thread, and a
    /// refusal is something the calling fiber can be told about and handle.
    /// So the ceiling here is `RLIMIT_NPROC`, `threads-max` and the memory for
    /// the stacks — the limits the operator set — reported where they bind
    /// rather than guessed at in advance.
    pub(in crate::io) fn submit(
        &mut self,
        id: SubmissionId,
        op: PoolOp,
        bounds: Bounds,
    ) -> Result<(), String> {
        // The pool carries the id as an opaque round-tripped token. The kind
        // travels with it so the completion can be checked against the entry the
        // id resolves through — a submission table that has drifted is then a
        // report rather than a wrong-arm free (see `OpKind`).
        let raw_id = id.as_u64();
        let kind = op.kind();
        let sender = self.sender();
        let eventfd = self.eventfd();
        let started = std::thread::Builder::new().spawn(move || {
            let id = raw_id;
            // Block every asynchronous signal on this worker so the kernel
            // never selects it as the delivery target for a watched POSIX
            // signal. The fault set stays deliverable.
            // See src/io/sigfd.rs and docs/posix-signals.md.
            crate::io::sigfd::mask_all_signals_on_this_thread();
            let (result_code, data) = run(op, bounds);
            publish_completion(
                &sender,
                eventfd,
                RawCompletion::Pool(PoolCompletion {
                    id,
                    kind,
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
                // A refused spawn drops the closure it was given, and the
                // `Bounds` inside close the stop pipe's read end with them. The
                // write end is the hub's, so retire it here.
                self.forget_stop(id);
                Err(format!("async I/O: cannot start a worker thread: {}", e))
            }
        }
    }
}

/// Run one operation to its result, on the worker thread that owns it.
///
/// Each arm builds the bound the operation runs under, and which constructor it
/// picks is the operation's whole relationship with its descriptor.
/// `OpBound::new` reads or writes the descriptor, so it holds it non-blocking;
/// `OpBound::watching` only polls one somebody else owns, so it changes nothing;
/// `OpBound::detached` has no descriptor at all — a timer, an open, a child
/// wait, and a connect whose socket is not open yet.
///
/// `Task` and `Resolve` take no bound because nothing can bound them; their
/// submissions say so with `Bounds::uninterruptible`, and dropping the bounds
/// here is what disposes of them.
fn run(op: PoolOp, bounds: Bounds) -> (i32, Vec<u8>) {
    match op {
        PoolOp::Read { fd, size } => stream::read(OpBound::new(fd, bounds), fd, size),
        PoolOp::ReadExact {
            fd,
            size,
            graphemes,
            gen,
            held,
        } => stream::read_exact(OpBound::new(fd, bounds), fd, size, graphemes, gen, &held),
        PoolOp::ReadLine { fd } => stream::read_until(OpBound::new(fd, bounds), fd, true),
        PoolOp::ReadAll { fd } => stream::read_until(OpBound::new(fd, bounds), fd, false),
        PoolOp::Write { fd, data } => stream::write(OpBound::new(fd, bounds), fd, data),
        PoolOp::Flush { fd } => stream::flush(fd),

        PoolOp::Accept { fd } => net::accept(OpBound::new(fd, bounds), fd),
        PoolOp::RecvFrom { fd, size } => net::recv_from(OpBound::new(fd, bounds), fd, size),
        PoolOp::ConnectTcp { addr, options } => net::connect_tcp(addr, &options, bounds),
        PoolOp::ConnectUnix { path, options } => net::connect_unix(&path, &options, bounds),
        PoolOp::SendTo {
            fd,
            addr,
            port,
            data,
        } => net::send_to(fd, &addr, port, &data),
        PoolOp::Shutdown { fd, how } => net::shutdown(fd, how),

        // Either ending reports the same completion: a stopped timer's result
        // is discarded, and a fiber that wanted the elapsed timer cannot tell
        // the two apart anyway.
        PoolOp::Sleep => {
            let _ = OpBound::detached(bounds).sleep();
            (0, Vec::new())
        }
        PoolOp::ProcessWait { pid } => child::process_wait(OpBound::detached(bounds), pid),
        PoolOp::Open { path, flags, mode } => {
            open::open(OpBound::detached(bounds), &path, flags, mode)
        }
        PoolOp::Task(closure) => closure(),
        PoolOp::Resolve { hostname } => resolve(&hostname),

        PoolOp::WatchRead { fd } => event::watch_read(OpBound::new(fd, bounds), fd),
        #[cfg(any(target_os = "linux", target_os = "android"))]
        PoolOp::SigfdRead { fd, trace } => event::sigfd_read(OpBound::new(fd, bounds), &trace, fd),
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        PoolOp::SigfdRead { .. } => (
            -libc::ENOTSUP,
            b"sig-next: signalfd not supported on this platform".to_vec(),
        ),
        #[cfg(target_os = "macos")]
        PoolOp::KqSigRead { fd, signals, trace } => {
            event::kq_sig_read(OpBound::new(fd, bounds), &trace, fd, &signals)
        }
        #[cfg(not(target_os = "macos"))]
        PoolOp::KqSigRead { .. } => (
            -libc::ENOTSUP,
            b"sig-next: kqueue signal mode not supported on this platform".to_vec(),
        ),
        PoolOp::PollFd { fd, events } => {
            poll_fd(OpBound::watching(fd, bounds), events as libc::c_short)
        }
    }
}

/// Wait for `events` on a bare descriptor and report the mask that fired.
///
/// `ev/poll-fd` reports 0 when its timeout elapses rather than raising, which
/// is what lets a caller poll in a loop; the completion turns the expiry back
/// into that 0. The bound is what watches the stop pipe alongside the
/// descriptor, so `io/cancel` and `ev/timeout` reach a park that would
/// otherwise run to the caller's whole timeout.
fn poll_fd(bound: OpBound, events: libc::c_short) -> (i32, Vec<u8>) {
    match bound.wait_revents(events) {
        (Wake::Stopped, _) => (-libc::ECANCELED, Vec::new()),
        (Wake::TimedOut, _) => (-libc::ETIMEDOUT, Vec::new()),
        (Wake::Ready, revents) => (revents as i32, Vec::new()),
    }
}

/// Resolve a hostname to its addresses.
///
/// `getaddrinfo(3)` cannot be interrupted once entered, so this runs to the
/// resolver's own end however long that takes — through every retry the
/// resolver's configuration asks for. `io/cancel` discards the result; it does
/// not give the worker thread back any sooner. That is why the submission
/// declares `Bounds::uninterruptible` rather than taking a stop pipe it could
/// not poll.
fn resolve(hostname: &str) -> (i32, Vec<u8>) {
    use std::net::ToSocketAddrs;
    // getaddrinfo needs a "host:port" string; port 0 gets all addresses.
    match (hostname, 0u16).to_socket_addrs() {
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
