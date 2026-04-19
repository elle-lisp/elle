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
    use std::os::unix::io::RawFd;
    use std::path::{Path, PathBuf};

    pub(crate) struct FsWatcher {
        inner: RefCell<FsWatcherInner>,
    }

    struct FsWatcherInner {
        fd: RawFd,
        wd_to_path: HashMap<i32, PathBuf>,
        path_to_wd: HashMap<PathBuf, i32>,
        closed: bool,
    }

    impl FsWatcher {
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
            if inner.closed {
                return Err("watch-add: watcher is closed".into());
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
                    "watch-add: failed for \"{}\": {}",
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
            if inner.closed {
                return Err("watch-remove: watcher is closed".into());
            }
            let wd = inner
                .path_to_wd
                .remove(&path)
                .ok_or_else(|| format!("watch-remove: not watched: \"{}\"", path.display()))?;
            inner.wd_to_path.remove(&wd);
            unsafe { libc::inotify_rm_watch(inner.fd, wd as _) };
            Ok(())
        }

        pub fn raw_fd(&self) -> Result<RawFd, String> {
            let inner = self.inner.borrow();
            if inner.closed {
                return Err("watcher is closed".into());
            }
            Ok(inner.fd)
        }

        pub fn close(&self) {
            let mut inner = self.inner.borrow_mut();
            if !inner.closed {
                unsafe { libc::close(inner.fd) };
                inner.closed = true;
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
}

// ─── macOS: kqueue ─────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod platform {
    use super::{WatchEvent, WatchEventKind};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::os::unix::io::RawFd;
    use std::path::{Path, PathBuf};

    pub(crate) struct FsWatcher {
        inner: RefCell<FsWatcherInner>,
    }

    struct FsWatcherInner {
        kq: RawFd,
        /// fd → watched path (we open each watched path to get an fd for kqueue)
        fd_to_path: HashMap<RawFd, PathBuf>,
        path_to_fd: HashMap<PathBuf, RawFd>,
        closed: bool,
    }

    impl FsWatcher {
        pub fn new() -> Result<Self, String> {
            let kq = unsafe { libc::kqueue() };
            if kq < 0 {
                return Err(format!(
                    "kqueue failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            // Set close-on-exec
            unsafe { libc::fcntl(kq, libc::F_SETFD, libc::FD_CLOEXEC) };
            Ok(FsWatcher {
                inner: RefCell::new(FsWatcherInner {
                    kq,
                    fd_to_path: HashMap::new(),
                    path_to_fd: HashMap::new(),
                    closed: false,
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
            if inner.closed {
                return Err("watch-add: watcher is closed".into());
            }
            let c_path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
                .map_err(|_| "watch-add: path contains null byte".to_string())?;
            // Open the path to get an fd for kqueue EVFILT_VNODE
            let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_EVTONLY | libc::O_CLOEXEC) };
            if fd < 0 {
                return Err(format!(
                    "watch-add: open failed for \"{}\": {}",
                    path.display(),
                    std::io::Error::last_os_error()
                ));
            }
            // Register the fd with kqueue
            let fflags = libc::NOTE_WRITE
                | libc::NOTE_DELETE
                | libc::NOTE_RENAME
                | libc::NOTE_EXTEND
                | libc::NOTE_ATTRIB;
            let changelist = [libc::kevent {
                ident: fd as libc::uintptr_t,
                filter: libc::EVFILT_VNODE,
                flags: libc::EV_ADD | libc::EV_CLEAR,
                fflags,
                data: 0,
                udata: std::ptr::null_mut(),
            }];
            let ret = unsafe {
                libc::kevent(
                    inner.kq,
                    changelist.as_ptr(),
                    1,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null(),
                )
            };
            if ret < 0 {
                unsafe { libc::close(fd) };
                return Err(format!(
                    "watch-add: kevent failed for \"{}\": {}",
                    path.display(),
                    std::io::Error::last_os_error()
                ));
            }
            inner.fd_to_path.insert(fd, path.to_path_buf());
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
            if inner.closed {
                return Err("watch-remove: watcher is closed".into());
            }
            let fd = inner
                .path_to_fd
                .remove(&path)
                .ok_or_else(|| format!("watch-remove: not watched: \"{}\"", path.display()))?;
            inner.fd_to_path.remove(&fd);
            // EV_DELETE removes it from the kqueue; closing the fd also does it
            unsafe { libc::close(fd) };
            Ok(())
        }

        /// Get the kqueue fd for thread-pool blocking kevent() call.
        pub fn raw_fd(&self) -> Result<RawFd, String> {
            let inner = self.inner.borrow();
            if inner.closed {
                return Err("watcher is closed".into());
            }
            Ok(inner.kq)
        }

        pub fn close(&self) {
            let mut inner = self.inner.borrow_mut();
            if !inner.closed {
                // Close all watched fds
                for &fd in inner.fd_to_path.keys() {
                    unsafe { libc::close(fd) };
                }
                unsafe { libc::close(inner.kq) };
                inner.closed = true;
                inner.fd_to_path.clear();
                inner.path_to_fd.clear();
            }
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

                let kind = if fflags & libc::NOTE_DELETE as u32 != 0 {
                    WatchEventKind::Remove
                } else if fflags & libc::NOTE_RENAME as u32 != 0 {
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

    impl Drop for FsWatcher {
        fn drop(&mut self) {
            let inner = self.inner.get_mut();
            if !inner.closed {
                for &fd in inner.fd_to_path.keys() {
                    unsafe { libc::close(fd) };
                }
                unsafe { libc::close(inner.kq) };
                inner.closed = true;
            }
        }
    }

    impl std::fmt::Debug for FsWatcher {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let inner = self.inner.borrow();
            write!(
                f,
                "FsWatcher(kq={}, paths={}, closed={})",
                inner.kq,
                inner.path_to_fd.len(),
                inner.closed
            )
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
pub(crate) use platform::FsWatcher;
