//! Socket and datagram submission paths (Accept / SendTo / RecvFrom / Shutdown),
//! split out of `AsyncBackend::submit`.

use super::*;
use crate::io::pool::BufferHandle;

impl AsyncBackend {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn submit_socket(
        inner: &mut AsyncBackendInner,
        request: &IoRequest,
        id: SubmissionId,
        fd: std::os::unix::io::RawFd,
        port_key: PortKey,
        port: &Port,
        buf_handle: Option<BufferHandle>,
    ) -> Result<SubmissionId, String> {
        match &request.op {
            IoOp::Accept {
                ref options,
                encoding,
                ref accept_port,
            } => {
                let listener_kind = Some(port.kind());

                let AsyncBackendInner {
                    ref mut platform,
                    ref mut hub,
                    ref mut pending,
                    ..
                } = *inner;

                match platform {
                    #[cfg(target_os = "linux")]
                    PlatformBackend::Uring(ring) => {
                        crate::io::uring::submit_uring_accept(ring, id, fd, request.timeout)?;
                    }
                    PlatformBackend::ThreadPool => {
                        hub.submit(id, PoolOp::Accept { fd })?;
                    }
                }

                pending.insert(
                    id,
                    PendingOp::Port {
                        op: IoOp::Accept {
                            options: options.clone(),
                            encoding: *encoding,
                            accept_port: *accept_port,
                        },
                        port_key,
                        port: request.port,
                        buffer_handle: buf_handle,
                        listener_kind,
                        filled: 0,
                    },
                );
                Ok(id)
            }
            IoOp::SendTo {
                ref addr,
                port_num,
                ref data,
            } => {
                let bytes = Self::extract_write_bytes(data);

                let AsyncBackendInner {
                    ref mut platform,
                    ref mut hub,
                    ref mut pending,
                    ref mut buffer_pool,
                    ..
                } = *inner;

                match platform {
                    #[cfg(target_os = "linux")]
                    PlatformBackend::Uring(ring) => {
                        let payload = format!("{}:{}\0", addr, port_num).into_bytes();
                        let mut full_payload = payload;
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
                        hub.submit(
                            id,
                            PoolOp::SendTo {
                                fd,
                                addr: addr.clone(),
                                port: *port_num,
                                data: bytes,
                            },
                        )?;
                    }
                }

                pending.insert(
                    id,
                    PendingOp::Port {
                        op: IoOp::SendTo {
                            addr: addr.clone(),
                            port_num: *port_num,
                            data: *data,
                        },
                        port_key,
                        port: request.port,
                        buffer_handle: buf_handle,
                        listener_kind: None,
                        filled: 0,
                    },
                );
                Ok(id)
            }
            IoOp::RecvFrom { count, result } => {
                let AsyncBackendInner {
                    ref mut platform,
                    ref mut hub,
                    ref mut pending,
                    ref mut buffer_pool,
                    ..
                } = *inner;

                match platform {
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
                        let _ = buffer_pool;
                        hub.submit(id, PoolOp::RecvFrom { fd, size: *count })?;
                    }
                }

                pending.insert(
                    id,
                    PendingOp::Port {
                        op: IoOp::RecvFrom {
                            count: *count,
                            result: *result,
                        },
                        port_key,
                        port: request.port,
                        buffer_handle: buf_handle,
                        listener_kind: None,
                        filled: 0,
                    },
                );
                Ok(id)
            }
            IoOp::Shutdown { how } => {
                let AsyncBackendInner {
                    ref mut platform,
                    ref mut hub,
                    ref mut pending,
                    ref mut buffer_pool,
                    ..
                } = *inner;

                match platform {
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
                        hub.submit(id, PoolOp::Shutdown { fd, how: *how })?;
                    }
                }

                pending.insert(
                    id,
                    PendingOp::Port {
                        op: IoOp::Shutdown { how: *how },
                        port_key,
                        port: request.port,
                        buffer_handle: buf_handle,
                        listener_kind: None,
                        filled: 0,
                    },
                );
                Ok(id)
            }
            _ => unreachable!("submit_socket: non-socket op"),
        }
    }
}
