//! Filesystem watcher — inotify (Linux) / kqueue (macOS).
//!
//! Provides `FsWatcher`, a platform-specific event-driven filesystem watcher
//! that integrates with the async I/O subsystem. On Linux, the inotify fd can
//! be submitted to io_uring via `opcode::Read`. On macOS, the kqueue fd is
//! read via a blocking `kevent` call on the thread pool.

use std::path::PathBuf;

/// A filesystem event parsed from kernel notifications.
#[derive(Debug, Clone)]
pub(crate) struct WatchEvent {
    pub kind: WatchEventKind,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum WatchEventKind {
    /// inotify `IN_CREATE` (Linux/Android). kqueue's `EVFILT_VNODE` has no
    /// create notification for a watched file, so this is never constructed
    /// on macOS/BSD — but it stays in `as_keyword` for a uniform mapping.
    #[cfg_attr(not(any(target_os = "linux", target_os = "android")), allow(dead_code))]
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

// ─── Linux: inotify ────────────────────────────────────────────────────

#[cfg(any(target_os = "linux", target_os = "android"))]
mod platform {
    use super::{WatchEvent, WatchEventKind};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd, RawFd};
    use std::path::{Path, PathBuf};

    pub(crate) struct FsWatcher {
        inner: RefCell<FsWatcherInner>,
    }

    struct FsWatcherInner {
        /// The inotify fd. `None` once the watcher is closed; the `OwnedFd`
        /// closes the descriptor on drop, so there is no manual `close`
        /// call and no risk of a double-close. (`wd_*` hold inotify *watch
        /// descriptors*, not file descriptors — removed via `inotify_rm_watch`.)
        fd: Option<OwnedFd>,
        wd_to_path: HashMap<i32, PathBuf>,
        path_to_wd: HashMap<PathBuf, i32>,
    }

    impl FsWatcher {
        pub fn new() -> Result<Self, String> {
            let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
            if fd < 0 {
                return Err(crate::io::os_error("inotify_init1 failed"));
            }
            Ok(FsWatcher {
                inner: RefCell::new(FsWatcherInner {
                    // SAFETY: fresh inotify fd we own.
                    fd: Some(unsafe { OwnedFd::from_raw_fd(fd) }),
                    wd_to_path: HashMap::new(),
                    path_to_wd: HashMap::new(),
                }),
            })
        }

        pub fn add(&self, path: &str, recursive: bool) -> Result<(), String> {
            let path = Path::new(path)
                .canonicalize()
                .map_err(|e| format!("watch-add: cannot resolve \"{}\": {}", path, e))?;
            self.add_single(&path)?;
            if recursive && path.is_dir() {
                self.add_recursive(&path)?;
            }
            Ok(())
        }

        fn add_single(&self, path: &Path) -> Result<(), String> {
            let mut inner = self.inner.borrow_mut();
            let watch_fd = match &inner.fd {
                Some(fd) => fd.as_raw_fd(),
                None => return Err("watch-add: watcher is closed".into()),
            };
            let c_path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
                .map_err(|_| "watch-add: path contains null byte".to_string())?;
            let mask = libc::IN_MODIFY
                | libc::IN_CREATE
                | libc::IN_DELETE
                | libc::IN_MOVED_FROM
                | libc::IN_MOVED_TO;
            let wd = unsafe { libc::inotify_add_watch(watch_fd, c_path.as_ptr(), mask) };
            if wd < 0 {
                return Err(crate::io::os_error(&format!(
                    "watch-add: failed for \"{}\"",
                    path.display()
                )));
            }
            inner.wd_to_path.insert(wd, path.to_path_buf());
            inner.path_to_wd.insert(path.to_path_buf(), wd);
            Ok(())
        }

        fn add_recursive(&self, dir: &Path) -> Result<(), String> {
            let entries = std::fs::read_dir(dir)
                .map_err(|e| format!("watch-add: cannot read \"{}\": {}", dir.display(), e))?;
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    self.add_single(&p)?;
                    self.add_recursive(&p)?;
                }
            }
            Ok(())
        }

        pub fn remove(&self, path: &str) -> Result<(), String> {
            let path = Path::new(path)
                .canonicalize()
                .map_err(|e| format!("watch-remove: cannot resolve \"{}\": {}", path, e))?;
            let mut inner = self.inner.borrow_mut();
            let watch_fd = match &inner.fd {
                Some(fd) => fd.as_raw_fd(),
                None => return Err("watch-remove: watcher is closed".into()),
            };
            let wd = inner
                .path_to_wd
                .remove(&path)
                .ok_or_else(|| format!("watch-remove: not watched: \"{}\"", path.display()))?;
            inner.wd_to_path.remove(&wd);
            unsafe { libc::inotify_rm_watch(watch_fd, wd as _) };
            Ok(())
        }

        pub fn raw_fd(&self) -> Result<RawFd, String> {
            let inner = self.inner.borrow();
            match &inner.fd {
                Some(fd) => Ok(fd.as_raw_fd()),
                None => Err("watcher is closed".into()),
            }
        }

        pub fn close(&self) {
            let mut inner = self.inner.borrow_mut();
            // Dropping the OwnedFd closes the inotify descriptor.
            if inner.fd.take().is_some() {
                inner.wd_to_path.clear();
                inner.path_to_wd.clear();
            }
        }

        pub fn parse_events(&self, buf: &[u8]) -> Vec<WatchEvent> {
            let inner = self.inner.borrow();
            let mut events = Vec::new();
            let mut offset = 0;
            let event_size = std::mem::size_of::<libc::inotify_event>();

            while offset + event_size <= buf.len() {
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

                let base_path = inner.wd_to_path.get(&raw.wd).cloned().unwrap_or_default();
                let file_name = if name_len > 0 {
                    let name_bytes = &buf[offset + event_size..offset + event_size + name_len];
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
    }

    // No explicit Drop: the `Option<OwnedFd>` field closes the inotify
    // descriptor when the FsWatcher is dropped.

    impl std::fmt::Debug for FsWatcher {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let inner = self.inner.borrow();
            write!(
                f,
                "FsWatcher(fd={:?}, paths={}, closed={})",
                inner.fd.as_ref().map(|fd| fd.as_raw_fd()),
                inner.path_to_wd.len(),
                inner.fd.is_none()
            )
        }
    }

    #[cfg(test)]
    mod tests {
        use super::FsWatcher;

        /// The inotify fd is live until `close`, gone after, and `close`
        /// is idempotent — the `Option<OwnedFd>` must never double-close.
        #[test]
        fn fd_lifecycle_across_close_is_safe() {
            let w = FsWatcher::new().expect("inotify_init1");
            assert!(w.raw_fd().expect("open watcher has an fd") >= 0);

            // Exercise the watch-descriptor maps with a real path.
            let dir = std::env::temp_dir();
            let dir = dir.to_str().unwrap();
            w.add(dir, false).expect("watch temp dir");
            w.remove(dir).expect("unwatch temp dir");

            // close drops the OwnedFd → raw_fd now reports closed.
            w.close();
            assert!(w.raw_fd().is_err(), "closed watcher exposes no fd");
            // Idempotent: a second close must not double-close / panic.
            w.close();
            // Operations after close are rejected, not UB on a stale fd.
            assert!(w.add(dir, false).is_err());
        }
    }
}

// ─── macOS: kqueue ─────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod platform {
    use super::{WatchEvent, WatchEventKind};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd, RawFd};
    use std::path::{Path, PathBuf};

    pub(crate) struct FsWatcher {
        inner: RefCell<FsWatcherInner>,
    }

    struct FsWatcherInner {
        /// The kqueue. `None` once the watcher is closed; the `OwnedFd`
        /// closes the descriptor on drop.
        kq: Option<OwnedFd>,
        /// raw fd → watched path. The fds are *owned* by `path_to_fd`; this
        /// map keeps the raw value for reverse lookup in `parse_events`.
        fd_to_path: HashMap<RawFd, PathBuf>,
        /// watched path → the owned fd opened for it. Dropping an entry
        /// closes the fd, which also removes it from the kqueue.
        path_to_fd: HashMap<PathBuf, OwnedFd>,
    }

    impl FsWatcher {
        pub fn new() -> Result<Self, String> {
            let kq = unsafe { libc::kqueue() };
            if kq < 0 {
                return Err(crate::io::os_error("kqueue failed"));
            }
            // SAFETY: fresh kqueue fd we own.
            let kq = unsafe { OwnedFd::from_raw_fd(kq) };
            // Set close-on-exec
            unsafe { libc::fcntl(kq.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) };
            Ok(FsWatcher {
                inner: RefCell::new(FsWatcherInner {
                    kq: Some(kq),
                    fd_to_path: HashMap::new(),
                    path_to_fd: HashMap::new(),
                }),
            })
        }

        pub fn add(&self, path: &str, recursive: bool) -> Result<(), String> {
            let path = Path::new(path)
                .canonicalize()
                .map_err(|e| format!("watch-add: cannot resolve \"{}\": {}", path, e))?;
            self.add_single(&path)?;
            if recursive && path.is_dir() {
                self.add_recursive(&path)?;
            }
            Ok(())
        }

        fn add_single(&self, path: &Path) -> Result<(), String> {
            let mut inner = self.inner.borrow_mut();
            let kq_raw = match &inner.kq {
                Some(kq) => kq.as_raw_fd(),
                None => return Err("watch-add: watcher is closed".into()),
            };
            let c_path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
                .map_err(|_| "watch-add: path contains null byte".to_string())?;
            // Open the path to get an fd for kqueue EVFILT_VNODE
            let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_EVTONLY | libc::O_CLOEXEC) };
            if fd < 0 {
                return Err(crate::io::os_error(&format!(
                    "watch-add: open failed for \"{}\"",
                    path.display()
                )));
            }
            // SAFETY: fresh fd we own; the OwnedFd closes it on the kevent
            // failure path below and when the entry leaves `path_to_fd`.
            let fd = unsafe { OwnedFd::from_raw_fd(fd) };
            // Register the fd with kqueue
            let fflags = libc::NOTE_WRITE
                | libc::NOTE_DELETE
                | libc::NOTE_RENAME
                | libc::NOTE_EXTEND
                | libc::NOTE_ATTRIB;
            let changelist = [libc::kevent {
                ident: fd.as_raw_fd() as libc::uintptr_t,
                filter: libc::EVFILT_VNODE,
                flags: libc::EV_ADD | libc::EV_CLEAR,
                fflags,
                data: 0,
                udata: std::ptr::null_mut(),
            }];
            let ret = unsafe {
                libc::kevent(
                    kq_raw,
                    changelist.as_ptr(),
                    1,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null(),
                )
            };
            if ret < 0 {
                // `fd` drops here, closing it.
                return Err(crate::io::os_error(&format!(
                    "watch-add: kevent failed for \"{}\"",
                    path.display()
                )));
            }
            inner.fd_to_path.insert(fd.as_raw_fd(), path.to_path_buf());
            inner.path_to_fd.insert(path.to_path_buf(), fd);
            Ok(())
        }

        fn add_recursive(&self, dir: &Path) -> Result<(), String> {
            let entries = std::fs::read_dir(dir)
                .map_err(|e| format!("watch-add: cannot read \"{}\": {}", dir.display(), e))?;
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    self.add_single(&p)?;
                    self.add_recursive(&p)?;
                }
            }
            Ok(())
        }

        pub fn remove(&self, path: &str) -> Result<(), String> {
            let path = Path::new(path)
                .canonicalize()
                .map_err(|e| format!("watch-remove: cannot resolve \"{}\": {}", path, e))?;
            let mut inner = self.inner.borrow_mut();
            if inner.kq.is_none() {
                return Err("watch-remove: watcher is closed".into());
            }
            let fd = inner
                .path_to_fd
                .remove(&path)
                .ok_or_else(|| format!("watch-remove: not watched: \"{}\"", path.display()))?;
            inner.fd_to_path.remove(&fd.as_raw_fd());
            // Dropping the OwnedFd closes the fd, which also removes the
            // EVFILT_VNODE registration from the kqueue.
            Ok(())
        }

        /// Get the kqueue fd for thread-pool blocking kevent() call.
        pub fn raw_fd(&self) -> Result<RawFd, String> {
            let inner = self.inner.borrow();
            match &inner.kq {
                Some(kq) => Ok(kq.as_raw_fd()),
                None => Err("watcher is closed".into()),
            }
        }

        pub fn close(&self) {
            let mut inner = self.inner.borrow_mut();
            // Dropping the OwnedFds closes every watched fd and the kqueue.
            inner.path_to_fd.clear();
            inner.fd_to_path.clear();
            inner.kq = None;
        }

        /// Parse raw kevent results into WatchEvents.
        /// On macOS, the thread-pool WatchRead handler calls kevent() directly
        /// and encodes the results as a sequence of (fd:i32, fflags:u32) pairs.
        pub fn parse_events(&self, buf: &[u8]) -> Vec<WatchEvent> {
            let inner = self.inner.borrow();
            let entry_size = 4 + 4; // fd (i32) + fflags (u32)
            let mut events = Vec::new();
            let mut offset = 0;

            while offset + entry_size <= buf.len() {
                let fd = i32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap());
                let fflags = u32::from_le_bytes(buf[offset + 4..offset + 8].try_into().unwrap());
                offset += entry_size;

                let path = inner.fd_to_path.get(&fd).cloned().unwrap_or_default();

                let kind = if fflags & libc::NOTE_DELETE != 0 {
                    WatchEventKind::Remove
                } else if fflags & libc::NOTE_RENAME != 0 {
                    WatchEventKind::Rename
                } else {
                    // NOTE_WRITE, NOTE_EXTEND, NOTE_ATTRIB → Modify
                    WatchEventKind::Modify
                };

                events.push(WatchEvent { kind, path });
            }
            events
        }
    }

    // No explicit Drop: the `Option<OwnedFd>` kq and the owned fds in
    // `path_to_fd` all close when the FsWatcher is dropped.

    impl std::fmt::Debug for FsWatcher {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let inner = self.inner.borrow();
            write!(
                f,
                "FsWatcher(kq={:?}, paths={}, closed={})",
                inner.kq.as_ref().map(|kq| kq.as_raw_fd()),
                inner.path_to_fd.len(),
                inner.kq.is_none()
            )
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
pub(crate) use platform::FsWatcher;
