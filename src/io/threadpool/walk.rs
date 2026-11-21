// audited: 2026-11-20
//! The file operations a pool worker runs one entry at a time, asking before each step whether to stop.
//!
//! docs/impl/io/cancel.md
//! docs/io/files.md

use super::file::failed;
use std::ffi::{CStr, CString};
use std::io::ErrorKind;
use std::os::unix::io::RawFd;
use std::path::Path;

/// Why a walk ended before its last step.
pub(super) enum Halt {
    /// The check said stop.
    Stopped,
    /// A step failed.
    Failed(std::io::Error),
}

impl From<std::io::Error> for Halt {
    fn from(e: std::io::Error) -> Halt {
        Halt::Failed(e)
    }
}

/// A failure the OS reports as `errno`.
pub(super) fn errno(code: i32) -> Halt {
    Halt::Failed(std::io::Error::from_raw_os_error(code))
}

/// Ask `stop` before a step, and halt when it says so.
pub(super) fn step(stop: &mut dyn FnMut() -> bool) -> Result<(), Halt> {
    if stop() {
        Err(Halt::Stopped)
    } else {
        Ok(())
    }
}

/// What a walk reports: 0 and its answer's bytes, `-ECANCELED` when it was
/// stopped, as every stopped pool operation answers, or the failure.
pub(super) fn report(walked: Result<Vec<u8>, Halt>) -> (i32, Vec<u8>) {
    match walked {
        Ok(bytes) => (0, bytes),
        Err(Halt::Stopped) => (-libc::ECANCELED, Vec::new()),
        Err(Halt::Failed(e)) => failed(&e),
    }
}

/// The names in the directory `path`, each followed by a NUL, which no name
/// holds. A name that is not UTF-8 is left out, and an entry the OS fails to
/// read fails the whole list.
pub(super) fn list(path: &str, stop: &mut dyn FnMut() -> bool) -> (i32, Vec<u8>) {
    report(names(path, stop))
}

fn names(path: &str, stop: &mut dyn FnMut() -> bool) -> Result<Vec<u8>, Halt> {
    step(stop)?;
    let mut names = Vec::new();
    for entry in std::fs::read_dir(path)? {
        step(stop)?;
        if let Ok(name) = entry?.file_name().into_string() {
            names.extend_from_slice(name.as_bytes());
            names.push(0);
        }
    }
    Ok(names)
}

/// Remove `path` and everything under it, as `std::fs::remove_dir_all` does.
/// A symlink goes as a link, so nothing outside the tree goes, and a `path`
/// that is one loses the link alone. A regular file fails with `ENOTDIR`.
///
/// Each directory is held open and its entries named relative to it, and every
/// open refuses a symlink. So a directory swapped for a symlink mid-walk is
/// unlinked rather than entered. An entry another process removed first is
/// not a failure.
pub(super) fn delete_tree(path: &str, stop: &mut dyn FnMut() -> bool) -> (i32, Vec<u8>) {
    report(delete_top(path, stop).map(|()| Vec::new()))
}

fn delete_top(path: &str, stop: &mut dyn FnMut() -> bool) -> Result<(), Halt> {
    let name = CString::new(path).map_err(|_| errno(libc::EINVAL))?;
    step(stop)?;
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Ok(std::fs::remove_file(path)?);
    }
    delete_at(None, &name, stop)
}

/// Remove `name`, in the directory `parent` or else the working directory, and
/// everything under it.
fn delete_at(
    parent: Option<RawFd>,
    name: &CStr,
    stop: &mut dyn FnMut() -> bool,
) -> Result<(), Halt> {
    let at = parent.unwrap_or(libc::AT_FDCWD);
    let mut dir = match Dir::open(at, name) {
        Ok(dir) => dir,
        // Not a directory, or a symlink: an entry to unlink rather than enter.
        // The top of the walk is what the caller named, so there it fails.
        Err(e)
            if parent.is_some()
                && matches!(e.raw_os_error(), Some(libc::ENOTDIR | libc::ELOOP)) =>
        {
            return unless_gone(unlink_at(at, name, 0));
        }
        Err(e) => return Err(e.into()),
    };
    loop {
        step(stop)?;
        let Some((child, kind)) = dir.next()? else {
            break;
        };
        if matches!(child.as_bytes(), b"." | b"..") {
            continue;
        }
        let removed = match kind {
            libc::DT_DIR | libc::DT_UNKNOWN => delete_at(Some(dir.fd()), &child, stop),
            _ => unlink_at(dir.fd(), &child, 0).map_err(Halt::from),
        };
        match removed {
            Err(Halt::Failed(e)) if e.kind() == ErrorKind::NotFound => {}
            other => other?,
        }
    }
    drop(dir);
    unless_gone(unlink_at(at, name, libc::AT_REMOVEDIR))
}

/// A removal's result, with an entry already gone counted as removed.
fn unless_gone(r: std::io::Result<()>) -> Result<(), Halt> {
    match r {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        r => Ok(r?),
    }
}

fn unlink_at(at: RawFd, name: &CStr, flags: i32) -> std::io::Result<()> {
    if unsafe { libc::unlinkat(at, name.as_ptr(), flags) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Make the directory `path` and each missing ancestor, one at a time, as
/// `std::fs::create_dir_all` does. An ancestor already there as a directory,
/// or as a symlink to one, is fine, and so is one another process makes first.
pub(super) fn make_dirs(path: &str, stop: &mut dyn FnMut() -> bool) -> (i32, Vec<u8>) {
    report(make(Path::new(path), stop).map(|()| Vec::new()))
}

fn make(path: &Path, stop: &mut dyn FnMut() -> bool) -> Result<(), Halt> {
    // The path, then each parent in turn, the order `std` climbs in.
    let chain: Vec<&Path> = path
        .ancestors()
        .take_while(|p| !p.as_os_str().is_empty())
        .collect();
    // Climb to the deepest directory that is there or that a mkdir makes.
    let mut made = 0;
    loop {
        let Some(dir) = chain.get(made) else {
            return Ok(());
        };
        step(stop)?;
        match std::fs::create_dir(dir) {
            Ok(()) => break,
            Err(e) if e.kind() == ErrorKind::NotFound && made + 1 < chain.len() => made += 1,
            Err(_) if dir.is_dir() => break,
            Err(e) => return Err(e.into()),
        }
    }
    // Then make each one below it.
    for dir in chain[..made].iter().rev() {
        step(stop)?;
        match std::fs::create_dir(dir) {
            Err(e) if !dir.is_dir() => return Err(e.into()),
            _ => {}
        }
    }
    Ok(())
}

/// An open directory stream, closed with itself.
struct Dir(std::ptr::NonNull<libc::DIR>);

impl Dir {
    /// Open the directory `name` in `at`, refusing a symlink. `O_NONBLOCK`
    /// keeps the open of a fifo, which a stream with no entry types meets,
    /// from waiting for a writer before `O_DIRECTORY` refuses it.
    fn open(at: RawFd, name: &CStr) -> std::io::Result<Dir> {
        let flags = libc::O_RDONLY
            | libc::O_DIRECTORY
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | libc::O_CLOEXEC;
        let fd = loop {
            let fd = unsafe { libc::openat(at, name.as_ptr(), flags) };
            if fd >= 0 {
                break fd;
            }
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::EINTR) {
                return Err(e);
            }
        };
        match std::ptr::NonNull::new(unsafe { libc::fdopendir(fd) }) {
            Some(dir) => Ok(Dir(dir)),
            None => {
                let e = std::io::Error::last_os_error();
                // SAFETY: the stream never took the descriptor, so it is ours.
                unsafe { libc::close(fd) };
                Err(e)
            }
        }
    }

    fn fd(&self) -> RawFd {
        unsafe { libc::dirfd(self.0.as_ptr()) }
    }

    /// The next entry's name and type, or `None` at the end. `readdir(3)`
    /// answers null for both, and only a failure sets `errno`.
    fn next(&mut self) -> std::io::Result<Option<(CString, u8)>> {
        clear_errno();
        let ent = unsafe { libc::readdir(self.0.as_ptr()) };
        if ent.is_null() {
            let e = std::io::Error::last_os_error();
            return match e.raw_os_error() {
                Some(0) => Ok(None),
                _ => Err(e),
            };
        }
        // SAFETY: the entry stays valid until the next call on this stream.
        let ent = unsafe { &*ent };
        let name = unsafe { CStr::from_ptr(ent.d_name.as_ptr()) }.to_owned();
        Ok(Some((name, ent.d_type)))
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        // SAFETY: the stream is this value's alone, and closes its descriptor.
        unsafe { libc::closedir(self.0.as_ptr()) };
    }
}

/// Set this thread's `errno` to 0.
fn clear_errno() {
    // SAFETY: each names this thread's own `errno`.
    unsafe {
        #[cfg(target_os = "linux")]
        let at = libc::__errno_location();
        #[cfg(target_os = "android")]
        let at = libc::__errno();
        #[cfg(target_os = "macos")]
        let at = libc::__error();
        *at = 0;
    }
}
