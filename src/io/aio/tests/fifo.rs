//! audited: 2026-11-17
//! A fifo under the platform temp root that unlinks with itself, for the tests whose operation waits on its other end.
//!
//! docs/impl/io/cancel.md

use super::temp_path;

/// A fifo that unlinks with itself. Nothing opens either end until a test does.
pub(super) struct Fifo(pub(super) String);

impl Fifo {
    pub(super) fn new(tag: &str) -> Fifo {
        let path = temp_path(tag);
        let c_path = std::ffi::CString::new(path.as_str()).unwrap();
        assert_eq!(
            unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) },
            0,
            "mkfifo failed"
        );
        Fifo(path)
    }
}

impl Drop for Fifo {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).ok();
    }
}
