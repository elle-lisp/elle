//! audited: 2026-09-18
//! Completion processing for async I/O operations: one arm per operation
//! shape, each answering with a value it was handed or one it builds itself.
//!
//! src/io/AGENTS.md
//! docs/impl/io-inflight.md

use crate::io::pending::PendingOp;
use crate::io::pool::{BufferHandle, BufferPool};
use crate::io::request::PortOp;
use crate::io::types::{FdState, PortKey};
use crate::io::{Completion, SubmissionId};
use crate::port::{Encoding, Port, PortKind};
use crate::value::heap::TableKey;
use crate::value::Value;
use std::collections::HashMap;
use std::os::unix::io::AsRawFd;
use std::os::unix::io::{FromRawFd, OwnedFd, RawFd};

/// Set TCP_NODELAY on a TCP stream fd to disable Nagle's algorithm.
mod port;
use port::complete_port_op;

fn set_tcp_nodelay(fd: &OwnedFd) {
    unsafe {
        let opt: libc::c_int = 1;
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::IPPROTO_TCP,
            libc::TCP_NODELAY,
            &opt as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
    }
}

/// Convert an errno to a human-readable message via strerror.
fn errno_message(errno: i32) -> String {
    std::io::Error::from_raw_os_error(errno).to_string()
}

/// True when this errno says the caller's `:timeout` elapsed rather than the
/// operation failing.
///
/// Three paths arrive here. io_uring cancels an operation whose linked timeout
/// fired, which reports `ECANCELED`. A thread-pool worker whose own bounded
/// wait expired reports `ETIMEDOUT`, and so does a kernel that gave up on a
/// TCP handshake. The caller asked one question of all three, so they carry one
/// answer: the `:timeout` error kind rather than a generic `:io-error`.
fn is_timeout_errno(errno: i32) -> bool {
    errno == libc::ECANCELED || errno == libc::ETIMEDOUT
}

#[allow(clippy::too_many_arguments)]
pub(super) fn process_raw_completion(
    id: SubmissionId,
    result_code: i32,
    data: Vec<u8>,
    pending: &PendingOp,
    fd_states: &mut HashMap<PortKey, FdState>,
    buffer_pool: &mut BufferPool,
    buf_handle: Option<BufferHandle>,
    // The requesting instance's heap; completion values are born on it
    // (`crate::io::completion_heap_ptr`).
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
    // The owning VM's Unicode generation, forwarded to the port arm.
    gen: crate::segment::Generation,
) -> Completion {
    // Where this completion builds an answer it cannot be handed: one region on
    // that heap, whose reference the completion carries and hands over
    // (docs/impl/io-inflight.md). One completion, one birthplace, coined here so
    // no arm can reach for a second.
    let mut birth = crate::io::Birthplace::on(origin_heap);
    // Release the buffer back to the pool (if present — reads don't use BufferPool)
    if let Some(bh) = buf_handle {
        buffer_pool.release(bh);
    }

    match pending {
        PendingOp::ProcessWait { siginfo, exit, .. } => {
            // buffer_pool.release is already called at the top of process_raw_completion.

            if result_code < 0 {
                // A wait that finds no child answers from the record when this
                // process is holding the status: two waits on one child are
                // legal, and the loser is entitled to what the winner reaped
                // (src/io/AGENTS.md § "A reap is never wasted").
                if -result_code == libc::ECHILD {
                    if let Some(code) = exit.status() {
                        if !siginfo.is_null() {
                            // SAFETY: as below — allocated at submit, reclaimed
                            // once, and this arm is the single exit point.
                            unsafe { drop(Box::from_raw(*siginfo)) };
                        }
                        return Completion::ok(id, birth, Value::int(code as i64));
                    }
                }
                // Which syscall actually ran is what the `siginfo` allocation
                // says: the kernel fills one for `IORING_OP_WAITID`, and the
                // pool worker — which calls `waitpid(2)` itself — leaves it
                // null. Naming the other one sends a reader looking for a call
                // this platform never makes.
                let syscall = if siginfo.is_null() {
                    "waitpid"
                } else {
                    "waitid"
                };
                // On the uring path, reclaim siginfo before returning.
                if !siginfo.is_null() {
                    unsafe { drop(Box::from_raw(*siginfo)) };
                }
                let errno = -result_code;
                let msg = format!(
                    "subprocess/wait: {} failed: errno {} ({})",
                    syscall,
                    errno,
                    errno_message(errno)
                );
                return Completion::failed(id, birth, "exec-error", msg);
            }

            let exit_code: i32 = if siginfo.is_null() {
                // Thread pool path: exit code is encoded as 4-byte LE int in
                // data. The worker recorded it as it reaped, so this is a read
                // of what the record already holds.
                if data.len() >= 4 {
                    i32::from_le_bytes(data[..4].try_into().unwrap())
                } else {
                    result_code
                }
            } else {
                // io_uring path: exit status is in siginfo_t filled by the kernel.
                // Reclaim the siginfo_t allocation.
                // SAFETY: `siginfo` was allocated via Box::into_raw in submit_process_wait.
                // This completion arm is the single exit point — the CQE fires exactly once
                // per SQE. `result_code >= 0` is the kernel saying it filled the struct.
                let si = unsafe { Box::from_raw(*siginfo) };
                unsafe { crate::io::request::exit_code_from_siginfo(&si) }
            };

            // The child is reaped, so this status is the only one there will
            // be. Keeping it is what lets a later wait on the same handle
            // answer at all.
            exit.keep(exit_code);

            Completion::ok(id, birth, Value::int(exit_code as i64))
        }
        PendingOp::Sleep { .. } => {
            // Sleep completes with -ETIME (62) on io_uring, or 0 on thread pool.
            // Both are success for a timer.
            Completion::ok(id, birth, Value::NIL)
        }
        PendingOp::Open {
            path,
            port: port_val,
            ..
        } => {
            if result_code < 0 {
                let errno = -result_code;
                let is_timeout = is_timeout_errno(errno);
                let msg = if is_timeout {
                    "I/O operation timed out".to_string()
                } else {
                    let os_err = std::io::Error::from_raw_os_error(errno);
                    format!("port/open: {}: {}", path, os_err)
                };
                let error_type = if is_timeout { "timeout" } else { "io-error" };
                return Completion::failed(id, birth, error_type, msg);
            }
            // SAFETY: result_code is a valid fd returned by the kernel (>= 0).
            let fd = unsafe { OwnedFd::from_raw_fd(result_code) };
            // Fill the fd into the pre-allocated port (born in the solver's region).
            let port_ref = port_val
                .as_external::<Port>()
                .expect("PendingOp::Open port must be a Port");
            port_ref.set_fd(fd);
            Completion::ok(id, birth, *port_val)
        }
        PendingOp::Connect {
            connect_fd,
            port: port_val,
            ..
        } => {
            if result_code < 0 {
                let errno = -result_code;
                let is_timeout = is_timeout_errno(errno);
                let msg = if is_timeout {
                    "I/O operation timed out".to_string()
                } else {
                    format!("I/O error: {}", errno_message(errno))
                };
                let error_type = if is_timeout { "timeout" } else { "io-error" };
                return Completion::failed(id, birth, error_type, msg);
            }
            // Connect: fd comes from PendingOp (set at submission time).
            let fd = connect_fd.unwrap_or(result_code as RawFd);
            let fd = unsafe { OwnedFd::from_raw_fd(fd) };
            // Port was pre-allocated by the caller (with the requested
            // encoding already set on the Port — see prim_tcp_connect /
            // prim_unix_connect). Set the fd on the existing port; no
            // need to recreate it here.
            if matches!(
                port_val.as_external::<Port>().map(|p| p.kind()),
                Some(PortKind::TcpStream)
            ) {
                set_tcp_nodelay(&fd);
            }
            let port_ref = port_val
                .as_external::<Port>()
                .expect("PendingOp::Connect port must be a Port");
            port_ref.set_fd(fd);
            Completion::ok(id, birth, *port_val)
        }
        PendingOp::Task { .. } => {
            if result_code < 0 {
                let msg = String::from_utf8_lossy(&data).to_string();
                Completion::failed(id, birth, "task-error", msg)
            } else {
                let bytes = birth.alloc().bytes(data);
                Completion::ok(id, birth, bytes)
            }
        }
        PendingOp::WatchNext { watcher, .. } => {
            if result_code <= 0 {
                let msg = if result_code == 0 {
                    "watcher closed".to_string()
                } else {
                    format!(
                        "watch read error: {}",
                        std::io::Error::from_raw_os_error(-result_code)
                    )
                };
                return Completion::failed(id, birth, "io-error", msg);
            }
            // Parse inotify events from raw bytes
            let events = if let Some(w) = watcher.as_external::<crate::io::watch::FsWatcher>() {
                w.parse_events(&data[..result_code as usize])
            } else {
                Vec::new()
            };
            // Convert to Elle array of structs. One shared region (the
            // birthplace's) for the whole nested result: the strings live inside
            // the structs, which live inside the array.
            let answer = {
                let ctx = birth.alloc();
                let event_values: Vec<Value> = events
                    .iter()
                    .map(|ev| {
                        let mut fields = std::collections::BTreeMap::new();
                        fields.insert(
                            crate::value::heap::TableKey::keyword("kind"),
                            Value::keyword(ev.kind.as_keyword()),
                        );
                        let path = ctx.string(ev.path.to_string_lossy().as_ref());
                        fields.insert(crate::value::heap::TableKey::keyword("path"), path);
                        ctx.struct_from(fields)
                    })
                    .collect();
                ctx.array(event_values)
            };
            Completion::ok(id, birth, answer)
        }
        PendingOp::SigNext { receiver, .. } => {
            // The receiver's own instance trace cell gates these diagnostics
            // per-instance (the completion runs on the scheduler thread, off any VM).
            let recv = receiver.as_external::<crate::io::sigfd::SignalReceiver>();
            let trace = recv.map(|r| r.trace());
            if let Some(t) = &trace {
                crate::io::sigfd::posix_trace(
                    t,
                    format_args!(
                        "completion: SigNext id={} result_code={} data_len={}",
                        id,
                        result_code,
                        data.len()
                    ),
                );
            }
            if result_code <= 0 {
                let msg = if result_code == 0 {
                    "signal receiver closed".to_string()
                } else {
                    format!(
                        "sig-next read error: {}",
                        std::io::Error::from_raw_os_error(-result_code)
                    )
                };
                return Completion::failed(id, birth, "io-error", msg);
            }
            let events = if let Some(r) = recv {
                r.parse_events(&data[..result_code as usize])
            } else {
                Vec::new()
            };
            if let Some(t) = &trace {
                crate::io::sigfd::posix_trace(
                    t,
                    format_args!("completion: SigNext parsed {} events", events.len()),
                );
            }
            // One shared region (the birthplace's): each event struct lives
            // inside the array.
            let answer = {
                let ctx = birth.alloc();
                let event_values: Vec<Value> = events
                    .iter()
                    .map(|ev| {
                        let name =
                            crate::io::sigmap::signum_to_keyword(ev.signum).unwrap_or("unknown");
                        let mut fields = std::collections::BTreeMap::new();
                        fields.insert(
                            crate::value::heap::TableKey::keyword("signal"),
                            Value::keyword(name),
                        );
                        fields.insert(
                            crate::value::heap::TableKey::keyword("sender-pid"),
                            match ev.sender_pid {
                                Some(p) => Value::int(p as i64),
                                None => Value::NIL,
                            },
                        );
                        fields.insert(
                            crate::value::heap::TableKey::keyword("sender-uid"),
                            match ev.sender_uid {
                                Some(u) => Value::int(u as i64),
                                None => Value::NIL,
                            },
                        );
                        fields.insert(
                            crate::value::heap::TableKey::keyword("code"),
                            Value::int(ev.code as i64),
                        );
                        fields.insert(
                            crate::value::heap::TableKey::keyword("count"),
                            Value::int(ev.count as i64),
                        );
                        ctx.struct_from(fields)
                    })
                    .collect();
                ctx.array(event_values)
            };
            Completion::ok(id, birth, answer)
        }
        PendingOp::PollFd { .. } => {
            // result_code is the revents mask (positive) or negative errno.
            //
            // `ev/poll-fd` answers an expired wait with 0 rather than an error,
            // which is what lets a caller poll in a loop — `wayland/event-loop`
            // and `glib-wait` both do. Both backends report the expiry as an
            // errno, so both are mapped back to that 0 here: `ETIMEDOUT` from
            // the pool worker's bound, `ECANCELED` from the ring's linked
            // timeout, which cannot say which of the two ended the wait. A
            // genuine `io/cancel` also lands on `ECANCELED`, and its completion
            // is discarded before it reaches a fiber.
            if result_code < 0 {
                let errno = -result_code;
                if is_timeout_errno(errno) {
                    return Completion::ok(id, birth, Value::int(0));
                }
                let msg = format!("ev/poll-fd: poll error: errno {}", errno);
                return Completion::failed(id, birth, "io-error", msg);
            }
            Completion::ok(id, birth, Value::int(result_code as i64))
        }
        PendingOp::ChanSelectPark { .. } => {
            // The guard inside this PendingOp owns the fd(s) and the
            // wake-list registrations; its Drop runs when the caller
            // removes this PendingOp from the pending map after we
            // return.  We don't care which path fired (POLLIN, timeout,
            // or cancellation) — the wrapper distinguishes "got a
            // value" from "timed out" via chan/try-select + its own
            // deadline tracking.  Returning nil keeps the protocol
            // stateless.
            Completion::ok(id, birth, Value::NIL)
        }
        PendingOp::Resolve { .. } => {
            if result_code < 0 {
                let msg = if data.is_empty() {
                    "getaddrinfo: resolution failed".to_string()
                } else {
                    String::from_utf8_lossy(&data).to_string()
                };
                return Completion::failed(id, birth, "dns-error", msg);
            }
            // data contains newline-separated IP address strings. One shared
            // region (the birthplace's): each string lives inside the array.
            let ips_str = String::from_utf8_lossy(&data);
            let answer = {
                let ctx = birth.alloc();
                let ips: Vec<Value> = ips_str
                    .lines()
                    .filter(|s| !s.is_empty())
                    .map(|s| ctx.string(s))
                    .collect();
                ctx.array(ips)
            };
            Completion::ok(id, birth, answer)
        }
        // The port arm assembles its own answer, so it takes this completion's
        // birthplace rather than coining a second one.
        PendingOp::Port { .. } => {
            complete_port_op(id, result_code, data, pending, fd_states, birth, gen)
        }
    }
}
