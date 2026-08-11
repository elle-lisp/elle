//! Socket and datagram submission paths (Accept / SendTo / RecvFrom / Shutdown),
//! split out of `AsyncBackend::submit`.

use super::*;
use crate::io::pool::BufferHandle;

impl AsyncBackend {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn submit_socket(
        inner: &mut AsyncBackendInner,
        request: &IoRequest,
        op: &PortOp,
        id: SubmissionId,
        fd: std::os::unix::io::RawFd,
        port_key: PortKey,
        port: &Port,
        buf_handle: Option<BufferHandle>,
    ) -> Result<SubmissionId, String> {
        // Only Accept needs to know what it was listening on; the datagram and
        // shutdown paths leave it unset.
        let mut listener_kind = None;

        {
            let AsyncBackendInner {
                ref mut platform,
                ref mut hub,
                ref mut buffer_pool,
                ..
            } = *inner;

            match op {
                PortOp::Accept { .. } => {
                    listener_kind = Some(port.kind());
                    match platform {
                        #[cfg(target_os = "linux")]
                        PlatformBackend::Uring(ring) => {
                            crate::io::uring::submit_uring_accept(ring, id, fd, request.timeout)?;
                        }
                        PlatformBackend::ThreadPool => {
                            let _ = buffer_pool;
                            // An accept waits as long as no peer connects, so it
                            // takes both bounds every other open-ended pool op
                            // carries: the caller's deadline, and the stop pipe
                            // `hub.stop` writes — neither of which can reach a
                            // thread already inside `accept(2)`.
                            let bounds = hub.bounds(id, request.timeout);
                            hub.submit(id, PoolOp::Accept { fd }, bounds)?;
                        }
                    }
                }
                PortOp::SendTo {
                    ref addr,
                    port_num,
                    ref data,
                } => {
                    let bytes = Self::extract_write_bytes(data);
                    match platform {
                        #[cfg(target_os = "linux")]
                        PlatformBackend::Uring(ring) => {
                            // The uring path sends address and payload as one
                            // buffer: "host:port\0" then the bytes.
                            let mut full_payload = format!("{}:{}\0", addr, port_num).into_bytes();
                            full_payload.extend_from_slice(&bytes);
                            crate::io::uring::submit_uring_sendto(
                                ring,
                                id,
                                fd,
                                &full_payload,
                                request.timeout,
                                buffer_pool,
                            )?;
                        }
                        PlatformBackend::ThreadPool => {
                            let _ = buffer_pool;
                            // A datagram leaves for the network as soon as the
                            // kernel takes it; there is no peer to wait on.
                            hub.submit(
                                id,
                                PoolOp::SendTo {
                                    fd,
                                    addr: addr.clone(),
                                    port: *port_num,
                                    data: bytes,
                                },
                                Bounds::prompt(),
                            )?;
                        }
                    }
                }
                PortOp::RecvFrom { count, result } => match platform {
                    #[cfg(target_os = "linux")]
                    PlatformBackend::Uring(ring) => {
                        // Zero-copy: the iovec points straight at the pre-allocated
                        // `:data` buffer on the requesting fiber's heap.
                        crate::io::uring::submit_uring_recvfrom(
                            ring,
                            id,
                            fd,
                            *count,
                            result,
                            request.timeout,
                            buffer_pool,
                        )?;
                    }
                    PlatformBackend::ThreadPool => {
                        // The pool worker recvs into its own buffer and hands
                        // the bytes back through the hub, so neither the pool
                        // nor the destination value is needed at submit time.
                        let _ = buffer_pool;
                        let _ = result;
                        // A datagram socket waits on a sender the same way a
                        // listener waits on a caller, so the receive carries the
                        // same two bounds as the accept above.
                        let bounds = hub.bounds(id, request.timeout);
                        hub.submit(id, PoolOp::RecvFrom { fd, size: *count }, bounds)?;
                    }
                },
                PortOp::Shutdown { how } => match platform {
                    #[cfg(target_os = "linux")]
                    PlatformBackend::Uring(ring) => {
                        crate::io::uring::submit_uring_shutdown(
                            ring,
                            id,
                            fd,
                            *how,
                            request.timeout,
                            buffer_pool,
                        )?;
                    }
                    PlatformBackend::ThreadPool => {
                        let _ = buffer_pool;
                        // `shutdown(2)` tears the direction down and returns; it
                        // does not wait for the peer to notice.
                        hub.submit(id, PoolOp::Shutdown { fd, how: *how }, Bounds::prompt())?;
                    }
                },
                PortOp::ReadLine { .. }
                | PortOp::Read { .. }
                | PortOp::ReadExact { .. }
                | PortOp::ReadAll
                | PortOp::Write { .. }
                | PortOp::Flush => unreachable!("submit_socket: stream op"),
            }
        }

        inner.pending.insert(
            id,
            PendingOp::Port {
                op: op.clone(),
                port_key,
                port: request.port,
                buffer_handle: buf_handle,
                listener_kind,
                filled: 0,
                timeout: request.timeout,
            },
        );
        Ok(id)
    }
}
