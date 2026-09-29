//! audited: 2026-09-29
//! `Seek` and `Tell`: one `lseek(2)` each, answered inside the submit call.
//!
//! src/io/AGENTS.md

use super::*;

impl AsyncBackendInner {
    /// Answer `Seek` and `Tell` inside the submit call.
    ///
    /// `AsyncBackend::submit` calls this once it has the `PortKey` and before it
    /// reserves a buffer. Both operations are one `lseek(2)`, which does not
    /// block, so neither reaches io_uring or the thread pool.
    ///
    /// A seek clears the per-fd buffer, because the kernel offset and the
    /// logical position diverge otherwise. A tell leaves the buffer alone and
    /// answers the kernel offset less the bytes still buffered.
    pub(super) fn handle_seek_tell(
        &mut self,
        id: SubmissionId,
        port: &Port,
        port_key: &PortKey,
        op: &IoOp,
    ) -> Result<SubmissionId, String> {
        if port.kind() != PortKind::File {
            let err_msg = match op {
                IoOp::Seek { .. } => {
                    format!("port/seek: expected file port, got {:?}", port.kind())
                }
                IoOp::Tell => format!("port/tell: expected file port, got {:?}", port.kind()),
                _ => unreachable!(),
            };
            let birth = crate::io::Birthplace::on(self.origin_heap);
            self.completions
                .push_back(Completion::failed(id, birth, "type-error", err_msg));
            return Ok(id);
        }

        let result = match op {
            IoOp::Seek { offset, whence } => {
                // Discard buffered bytes — kernel offset and logical position diverge otherwise.
                if let Some(state) = self.fd_states.get_mut(port_key) {
                    state.buffer.clear();
                }
                port.with_fd(|fd| {
                    let raw = fd.as_raw_fd();
                    let ret = unsafe { libc::lseek(raw, *offset, *whence) };
                    if ret < 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(Value::int(ret as i64))
                    }
                })
                .unwrap_or_else(|| {
                    Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "port/seek: fd unavailable",
                    ))
                })
            }
            IoOp::Tell => {
                let buffer_len: i64 = self
                    .fd_states
                    .get(port_key)
                    .map(|state| state.buffer.len() as i64)
                    .unwrap_or(0);
                port.with_fd(|fd| {
                    let raw = fd.as_raw_fd();
                    let ret = unsafe { libc::lseek(raw, 0, libc::SEEK_CUR) };
                    if ret < 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(Value::int(ret as i64 - buffer_len))
                    }
                })
                .unwrap_or_else(|| {
                    Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "port/tell: fd unavailable",
                    ))
                })
            }
            _ => unreachable!(),
        };

        let mut birth = crate::io::Birthplace::on(self.origin_heap);
        let result = result.map_err(|e| birth.error("io-error", e.to_string()));
        self.completions
            .push_back(Completion::new(id, birth, result));
        Ok(id)
    }
}
