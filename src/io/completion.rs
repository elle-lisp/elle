//! Completion processing for async I/O operations.

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
    // Release the buffer back to the pool (if present — reads don't use BufferPool)
    if let Some(bh) = buf_handle {
        buffer_pool.release(bh);
    }

    match pending {
        PendingOp::ProcessWait {
            handle_val,
            siginfo,
            ..
        } => {
            // buffer_pool.release is already called at the top of process_raw_completion.

            if result_code < 0 {
                // On the uring path, reclaim siginfo before returning.
                if !siginfo.is_null() {
                    unsafe { drop(Box::from_raw(*siginfo)) };
                }
                let errno = -result_code;
                return Completion::err(
                    id,
                    crate::io::io_error(
                        "exec-error",
                        format!("subprocess/wait: waitid failed: errno {}", errno),
                        origin_heap,
                    ),
                );
            }

            let exit_code: i32 = if siginfo.is_null() {
                // Thread pool path: exit code is encoded as 4-byte LE int in data.
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
                // per SQE.
                let si = unsafe { Box::from_raw(*siginfo) };
                // si_code values for SIGCHLD:
                //   CLD_EXITED (1): si_status is exit code
                //   CLD_KILLED (2): si_status is signal number (return as negative)
                //   CLD_DUMPED (3): killed + core dump (return signal as negative)
                //
                // SAFETY: si is fully initialized (kernel wrote it on child exit;
                // result_code >= 0 confirms the waitid completed successfully).
                unsafe {
                    let si_code = si.si_code;
                    let si_status = si.si_status();
                    match si_code {
                        1 => si_status,      // CLD_EXITED: normal exit
                        2 | 3 => -si_status, // CLD_KILLED / CLD_DUMPED: negative signal number
                        _ => -1,             // unknown
                    }
                }
            };

            // Cache the exit code in the ProcessHandle.
            if let Some(handle) = handle_val.as_external::<crate::io::request::ProcessHandle>() {
                let mut state = handle.inner.borrow_mut();
                *state = crate::io::request::ProcessState::Exited(exit_code);
            }

            Completion::ok(id, Value::int(exit_code as i64))
        }
        PendingOp::Sleep { .. } => {
            // Sleep completes with -ETIME (62) on io_uring, or 0 on thread pool.
            // Both are success for a timer.
            Completion::ok(id, Value::NIL)
        }
        PendingOp::Open {
            path,
            port: port_val,
            ..
        } => {
            if result_code < 0 {
                let errno = -result_code;
                let is_timeout = errno == 125; // ECANCELED from linked timeout
                let msg = if is_timeout {
                    "I/O operation timed out".to_string()
                } else {
                    let os_err = std::io::Error::from_raw_os_error(errno);
                    format!("port/open: {}: {}", path, os_err)
                };
                let error_type = if is_timeout { "timeout" } else { "io-error" };
                return Completion::err(id, crate::io::io_error(error_type, msg, origin_heap));
            }
            // SAFETY: result_code is a valid fd returned by the kernel (>= 0).
            let fd = unsafe { OwnedFd::from_raw_fd(result_code) };
            // Fill the fd into the pre-allocated port (born in the solver's region).
            let port_ref = port_val
                .as_external::<Port>()
                .expect("PendingOp::Open port must be a Port");
            port_ref.set_fd(fd);
            Completion::ok(id, *port_val)
        }
        PendingOp::Connect {
            connect_fd,
            port: port_val,
            ..
        } => {
            if result_code < 0 {
                let errno = -result_code;
                let is_timeout = errno == 125;
                let msg = if is_timeout {
                    "I/O operation timed out".to_string()
                } else {
                    format!("I/O error: {}", errno_message(errno))
                };
                let error_type = if is_timeout { "timeout" } else { "io-error" };
                return Completion::err(id, crate::io::io_error(error_type, msg, origin_heap));
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
            Completion::ok(id, *port_val)
        }
        PendingOp::Task { .. } => {
            if result_code < 0 {
                let msg = String::from_utf8_lossy(&data).to_string();
                Completion::err(id, crate::io::io_error("task-error", msg, origin_heap))
            } else {
                let heap = unsafe { &mut *crate::io::completion_heap_ptr(origin_heap) };
                let ctx = crate::primitives::ctx::Alloc::new(heap);
                Completion::ok(id, ctx.bytes(data))
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
                return Completion::err(id, crate::io::io_error("io-error", msg, origin_heap));
            }
            // Parse inotify events from raw bytes
            let events = if let Some(w) = watcher.as_external::<crate::io::watch::FsWatcher>() {
                w.parse_events(&data[..result_code as usize])
            } else {
                Vec::new()
            };
            // Convert to Elle array of structs. One shared region (the ctx's) for
            // the whole nested result: the strings live inside the structs, which
            // live inside the array.
            let heap = unsafe { &mut *crate::io::completion_heap_ptr(origin_heap) };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let event_values: Vec<Value> = events
                .iter()
                .map(|ev| {
                    let mut fields = std::collections::BTreeMap::new();
                    fields.insert(
                        crate::value::heap::TableKey::Keyword("kind".into()),
                        Value::keyword(ev.kind.as_keyword()),
                    );
                    let path = ctx.string(ev.path.to_string_lossy().as_ref());
                    fields.insert(crate::value::heap::TableKey::Keyword("path".into()), path);
                    ctx.struct_from(fields)
                })
                .collect();
            Completion::ok(id, ctx.array(event_values))
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
                return Completion::err(id, crate::io::io_error("io-error", msg, origin_heap));
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
            // One shared region (the ctx's): each event struct lives inside the array.
            let heap = unsafe { &mut *crate::io::completion_heap_ptr(origin_heap) };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let event_values: Vec<Value> = events
                .iter()
                .map(|ev| {
                    let name = crate::io::sigmap::signum_to_keyword(ev.signum).unwrap_or("unknown");
                    let mut fields = std::collections::BTreeMap::new();
                    fields.insert(
                        crate::value::heap::TableKey::Keyword("signal".into()),
                        Value::keyword(name),
                    );
                    fields.insert(
                        crate::value::heap::TableKey::Keyword("sender-pid".into()),
                        match ev.sender_pid {
                            Some(p) => Value::int(p as i64),
                            None => Value::NIL,
                        },
                    );
                    fields.insert(
                        crate::value::heap::TableKey::Keyword("sender-uid".into()),
                        match ev.sender_uid {
                            Some(u) => Value::int(u as i64),
                            None => Value::NIL,
                        },
                    );
                    fields.insert(
                        crate::value::heap::TableKey::Keyword("code".into()),
                        Value::int(ev.code as i64),
                    );
                    fields.insert(
                        crate::value::heap::TableKey::Keyword("count".into()),
                        Value::int(ev.count as i64),
                    );
                    ctx.struct_from(fields)
                })
                .collect();
            Completion::ok(id, ctx.array(event_values))
        }
        PendingOp::PollFd { .. } => {
            // result_code is the revents mask (positive) or negative errno.
            if result_code < 0 {
                let errno = -result_code;
                let is_timeout = errno == 125; // ECANCELED from linked timeout
                let msg = if is_timeout {
                    "ev/poll-fd: timed out".to_string()
                } else {
                    format!("ev/poll-fd: poll error: errno {}", errno)
                };
                let error_type = if is_timeout { "timeout" } else { "io-error" };
                return Completion::err(id, crate::io::io_error(error_type, msg, origin_heap));
            }
            Completion::ok(id, Value::int(result_code as i64))
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
            Completion::ok(id, Value::NIL)
        }
        PendingOp::Resolve { .. } => {
            if result_code < 0 {
                let msg = if data.is_empty() {
                    "getaddrinfo: resolution failed".to_string()
                } else {
                    String::from_utf8_lossy(&data).to_string()
                };
                return Completion::err(id, crate::io::io_error("dns-error", msg, origin_heap));
            }
            // data contains newline-separated IP address strings. One shared
            // region (the ctx's): each string lives inside the array.
            let ips_str = String::from_utf8_lossy(&data);
            let heap = unsafe { &mut *crate::io::completion_heap_ptr(origin_heap) };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let ips: Vec<Value> = ips_str
                .lines()
                .filter(|s| !s.is_empty())
                .map(|s| ctx.string(s))
                .collect();
            Completion::ok(id, ctx.array(ips))
        }
        PendingOp::Port { .. } => {
            complete_port_op(id, result_code, data, pending, fd_states, origin_heap, gen)
        }
    }
}
