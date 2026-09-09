// audited: 2026-09-08
//! Where an image's bytes live: a descriptor, and the offset of the image's
//! first byte inside it.
//!
//! docs/impl/image/format.md

use std::fs::File;
use std::os::fd::{AsFd, BorrowedFd, FromRawFd};
use std::os::unix::fs::FileExt;
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
    /// On Linux the file is write-sealed before it is returned, so the
    /// immutability `MAP_PRIVATE` relies on is enforced by the kernel rather
    /// than by the caller's discipline.
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
#[cfg(target_os = "linux")]
fn anonymous_file(bytes: &[u8]) -> Result<File, ImageError> {
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
#[cfg(not(target_os = "linux"))]
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
    file.write_all_at(bytes, 0)?;
    Ok(file)
}
