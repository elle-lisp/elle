// audited: 2026-09-09
//! Where an image's bytes live: a descriptor, and the offset of the image's
//! first byte inside it.
//!
//! docs/impl/image/format.md

use std::fs::File;
use std::os::fd::{AsFd, BorrowedFd, FromRawFd};
use std::path::Path;

use crate::value::fiberheap::pagepool::base_page;

use super::ImageError;

/// A mappable image: an open descriptor and the offset the image starts at.
///
/// The hydrator takes one of these rather than a path, because not every
/// image has a path — the release build's blob lives inside the executable,
/// and an image that arrives as bytes is mapped from an anonymous memory file.
#[derive(Debug)]
pub struct ImageSource {
    file: File,
    offset: u64,
}

impl ImageSource {
    /// An image occupying a whole file.
    pub fn open(path: &Path) -> Result<ImageSource, ImageError> {
        ImageSource::at(File::open(path)?, 0)
    }

    /// An image starting at `offset` inside an already-open file.
    ///
    /// The offset must be a multiple of the OS base page: every page maps at
    /// this offset plus a base-page multiple, and `mmap` accepts no other
    /// file offset. A misaligned one is refused here, before anything maps.
    pub fn at(file: File, offset: u64) -> Result<ImageSource, ImageError> {
        let page = base_page();
        if !offset.is_multiple_of(page as u64) {
            return Err(ImageError::Unaligned { offset, page });
        }
        Ok(ImageSource { file, offset })
    }

    /// An image that arrived as bytes — over the network, from Redis, from a
    /// channel — in an anonymous memory file that no filesystem names.
    ///
    /// Where the kernel mints memory files, the file is write-sealed before it
    /// is returned, so the immutability `MAP_PRIVATE` relies on is enforced by
    /// the kernel rather than by the caller's discipline.
    pub fn from_bytes(bytes: &[u8]) -> Result<ImageSource, ImageError> {
        Ok(ImageSource {
            file: anonymous_file(bytes)?,
            offset: 0,
        })
    }

    /// The descriptor to map from.
    pub fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }

    /// The image's first byte, as an offset into that descriptor.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// The open file, for the hydrator's header read and its mappings.
    pub(crate) fn file(&self) -> &File {
        &self.file
    }
}

fn last_error() -> ImageError {
    ImageError::Io(std::io::Error::last_os_error())
}

/// A file holding `bytes` that no directory entry names.
///
/// The arm is chosen by the call the platform has, never by the name it goes
/// under: Rust spells Android's `target_os` as `"android"`, and Android's
/// kernel mints a memfd exactly as any other Linux one does.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn anonymous_file(bytes: &[u8]) -> Result<File, ImageError> {
    // Scoped to this arm: the other one holds a descriptor no `FileExt` method
    // can reach, so a module-level import would read as dead on that platform.
    use std::os::unix::fs::FileExt;

    // `MFD_ALLOW_SEALING` is what makes the seal below possible; a memfd
    // created without it can never be sealed.
    let name = b"elle-image\0";
    let fd = unsafe {
        libc::memfd_create(
            name.as_ptr() as *const libc::c_char,
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
        )
    };
    if fd < 0 {
        return Err(last_error());
    }
    // SAFETY: `memfd_create` answered with a descriptor this call owns.
    let file = unsafe { File::from_raw_fd(fd) };
    // A memfd is a file, so it takes its bytes the way a file does. The other
    // arm cannot: see `fill_through_a_mapping`.
    file.write_all_at(bytes, 0)?;

    // Seal writes and both size changes. A shrink would leave the mapping
    // reading past the end of the file, which faults with SIGBUS rather than
    // returning bytes — the one failure a `MAP_PRIVATE` view cannot absorb.
    let seals = libc::F_SEAL_WRITE | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW;
    if unsafe { libc::fcntl(fd, libc::F_ADD_SEALS, seals) } != 0 {
        return Err(last_error());
    }
    Ok(file)
}

/// A file holding `bytes` that no directory entry names.
///
/// macOS has neither `memfd_create` nor file seals, so this opens a POSIX
/// shared-memory object and unlinks it immediately: the name is gone before
/// the bytes are written, and the descriptor is the only way back to them.
/// Immutability is this process's discipline there rather than the kernel's.
///
/// Android takes the memfd arm above and never reaches this one: bionic
/// declares neither `shm_open` nor `shm_unlink`, so there is no POSIX shared
/// memory there to fall back to.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn anonymous_file(bytes: &[u8]) -> Result<File, ImageError> {
    use std::sync::atomic::{AtomicU64, Ordering};

    // A shared-memory name is process-global, so two hydrations racing on one
    // name would share bytes. The counter is what keeps them apart.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = format!(
        "/elle-image-{}-{}\0",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let cname = name.as_ptr() as *const libc::c_char;
    let fd = unsafe {
        libc::shm_open(
            cname,
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
            0o600 as libc::c_uint,
        )
    };
    if fd < 0 {
        return Err(last_error());
    }
    unsafe { libc::shm_unlink(cname) };
    // SAFETY: `shm_open` answered with a descriptor this call owns.
    let file = unsafe { File::from_raw_fd(fd) };
    if unsafe { libc::ftruncate(fd, bytes.len() as libc::off_t) } != 0 {
        return Err(last_error());
    }
    fill_through_a_mapping(&file, bytes)?;
    Ok(file)
}

/// Copy `bytes` into a descriptor that only `mmap` reaches.
///
/// A Darwin shared-memory object answers `mmap`, `ftruncate` and `fstat` and
/// refuses the rest, so the `pwrite` that fills a memfd fails on one with
/// `ESPIPE`. The bytes go in through a writable shared mapping, which is
/// dropped before the descriptor is handed on.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn fill_through_a_mapping(file: &File, bytes: &[u8]) -> Result<(), ImageError> {
    use std::os::fd::AsRawFd;

    // `mmap` refuses a zero length, and an empty image has nothing to copy.
    if bytes.is_empty() {
        return Ok(());
    }
    let addr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            bytes.len(),
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            file.as_raw_fd(),
            0,
        )
    };
    if addr == libc::MAP_FAILED {
        return Err(last_error());
    }
    // SAFETY: `mmap` answered with `bytes.len()` writable bytes at `addr`, and
    // a mapping this call just made cannot overlap the caller's slice.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len()) };
    if unsafe { libc::munmap(addr, bytes.len()) } != 0 {
        return Err(last_error());
    }
    Ok(())
}
