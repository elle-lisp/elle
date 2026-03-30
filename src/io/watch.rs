//! Filesystem watcher — inotify (Linux) / kqueue (macOS).
//!
//! Provides `FsWatcher`, a platform-specific event-driven filesystem watcher
//! that integrates with the async I/O subsystem. On Linux, the inotify fd can
//! be submitted to io_uring via `opcode::Read`. On macOS, the kqueue fd serves
//! the same role on the thread-pool path (blocking `kevent` call).

use std::cell::RefCell;
use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::path::{Path, PathBuf};

/// A filesystem event parsed from kernel notifications.
#[derive(Debug, Clone)]
pub(crate) struct WatchEvent {
    pub kind: WatchEventKind,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum WatchEventKind {
    Create,
    Modify,
    Remove,
    Rename,
}

impl WatchEventKind {
    pub fn as_keyword(&self) -> &'static str {
        match self {
            WatchEventKind::Create => "create",
            WatchEventKind::Modify => "modify",
            WatchEventKind::Remove => "remove",
            WatchEventKind::Rename => "rename",
        }
    }
}

pub(crate) struct FsWatcher {
    inner: RefCell<FsWatcherInner>,
}

struct FsWatcherInner {
    fd: RawFd,
    /// watch descriptor → base path
    wd_to_path: HashMap<i32, PathBuf>,
    /// base path → watch descriptor
    path_to_wd: HashMap<PathBuf, i32>,
    closed: bool,
}

impl FsWatcher {
    /// Create a new filesystem watcher.
    pub fn new() -> Result<Self, String> {
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(format!(
                "inotify_init1 failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(FsWatcher {
            inner: RefCell::new(FsWatcherInner {
                fd,
                wd_to_path: HashMap::new(),
                path_to_wd: HashMap::new(),
                closed: false,
            }),
        })
    }

    /// Add a path to the watcher. If recursive, walks subdirectories.
    pub fn add(&self, path: &str, recursive: bool) -> Result<(), String> {
        let path = Path::new(path)
            .canonicalize()
            .map_err(|e| format!("watch-add: cannot resolve path \"{}\": {}", path, e))?;
        self.add_single(&path)?;
        if recursive && path.is_dir() {
            self.add_recursive(&path)?;
        }
        Ok(())
    }

    fn add_single(&self, path: &Path) -> Result<(), String> {
        let mut inner = self.inner.borrow_mut();
        if inner.closed {
            return Err("watch-add: watcher is closed".to_string());
        }
        let c_path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
            .map_err(|_| "watch-add: path contains null byte".to_string())?;
        let mask = libc::IN_MODIFY
            | libc::IN_CREATE
            | libc::IN_DELETE
            | libc::IN_MOVED_FROM
            | libc::IN_MOVED_TO;
        let wd = unsafe { libc::inotify_add_watch(inner.fd, c_path.as_ptr(), mask) };
        if wd < 0 {
            return Err(format!(
                "watch-add: inotify_add_watch failed for \"{}\": {}",
                path.display(),
                std::io::Error::last_os_error()
            ));
        }
        inner.wd_to_path.insert(wd, path.to_path_buf());
        inner.path_to_wd.insert(path.to_path_buf(), wd);
        Ok(())
    }

    fn add_recursive(&self, dir: &Path) -> Result<(), String> {
        let entries = std::fs::read_dir(dir)
            .map_err(|e| format!("watch-add: cannot read dir \"{}\": {}", dir.display(), e))?;
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                self.add_single(&p)?;
                self.add_recursive(&p)?;
            }
        }
        Ok(())
    }

    /// Remove a watched path.
    pub fn remove(&self, path: &str) -> Result<(), String> {
        let path = Path::new(path)
            .canonicalize()
            .map_err(|e| format!("watch-remove: cannot resolve path \"{}\": {}", path, e))?;
        let mut inner = self.inner.borrow_mut();
        if inner.closed {
            return Err("watch-remove: watcher is closed".to_string());
        }
        let wd = inner
            .path_to_wd
            .remove(&path)
            .ok_or_else(|| format!("watch-remove: path not watched: \"{}\"", path.display()))?;
        inner.wd_to_path.remove(&wd);
        unsafe { libc::inotify_rm_watch(inner.fd, wd) };
        Ok(())
    }

    /// Get the raw fd for io_uring / thread-pool read submission.
    pub fn raw_fd(&self) -> Result<RawFd, String> {
        let inner = self.inner.borrow();
        if inner.closed {
            return Err("watcher is closed".to_string());
        }
        Ok(inner.fd)
    }

    /// Close the watcher fd.
    pub fn close(&self) {
        let mut inner = self.inner.borrow_mut();
        if !inner.closed {
            unsafe { libc::close(inner.fd) };
            inner.closed = true;
            inner.wd_to_path.clear();
            inner.path_to_wd.clear();
        }
    }

    /// Parse raw bytes from a read() on the inotify fd into WatchEvents.
    pub fn parse_events(&self, buf: &[u8]) -> Vec<WatchEvent> {
        let inner = self.inner.borrow();
        parse_inotify_events(buf, &inner.wd_to_path)
    }
}

impl Drop for FsWatcher {
    fn drop(&mut self) {
        let inner = self.inner.get_mut();
        if !inner.closed {
            unsafe { libc::close(inner.fd) };
            inner.closed = true;
        }
    }
}

impl std::fmt::Debug for FsWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.borrow();
        write!(
            f,
            "FsWatcher(fd={}, paths={}, closed={})",
            inner.fd,
            inner.path_to_wd.len(),
            inner.closed
        )
    }
}

/// Parse raw inotify_event structs from a buffer.
fn parse_inotify_events(buf: &[u8], wd_map: &HashMap<i32, PathBuf>) -> Vec<WatchEvent> {
    let mut events = Vec::new();
    let mut offset = 0;
    let event_size = std::mem::size_of::<libc::inotify_event>();

    while offset + event_size <= buf.len() {
        // SAFETY: we checked bounds above; inotify guarantees aligned structs
        let raw = unsafe { &*(buf.as_ptr().add(offset) as *const libc::inotify_event) };
        let name_len = raw.len as usize;
        if offset + event_size + name_len > buf.len() {
            break;
        }

        let kind = if raw.mask & libc::IN_CREATE != 0 {
            WatchEventKind::Create
        } else if raw.mask & libc::IN_MODIFY != 0 {
            WatchEventKind::Modify
        } else if raw.mask & libc::IN_DELETE != 0 {
            WatchEventKind::Remove
        } else if raw.mask & (libc::IN_MOVED_FROM | libc::IN_MOVED_TO) != 0 {
            WatchEventKind::Rename
        } else {
            offset += event_size + name_len;
            continue;
        };

        let base_path = wd_map.get(&raw.wd).cloned().unwrap_or_default();
        let file_name = if name_len > 0 {
            let name_bytes = &buf[offset + event_size..offset + event_size + name_len];
            // name is null-terminated; strip trailing nulls
            let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(name_len);
            String::from_utf8_lossy(&name_bytes[..end]).to_string()
        } else {
            String::new()
        };

        let path = if file_name.is_empty() {
            base_path
        } else {
            base_path.join(&file_name)
        };

        events.push(WatchEvent { kind, path });
        offset += event_size + name_len;
    }

    events
}
