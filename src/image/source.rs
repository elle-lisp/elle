// audited: 2026-09-09
//! Where an image's bytes live: a descriptor, and the offset of the image's
//! first byte inside it.
//!
//! docs/impl/image/format.md

use std::fs::File;
use std::os::fd::{AsFd, BorrowedFd};
use std::path::Path;

use crate::value::fiberheap::pagepool::base_page;

use super::ImageError;

/// A mappable image: an open descriptor and the offset the image starts at.
///
/// The hydrator takes one of these rather than a path, because not every
/// image has a path — the release build's blob lives inside the executable,
/// and an image that arrives as bytes is mapped from an anonymous file.
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

    /// The open file, for the hydrator's size check and its page mappings.
    pub(crate) fn file(&self) -> &File {
        &self.file
    }

    /// Fill `buf` from the descriptor, starting at `at`.
    ///
    /// One implementation for every source, because every descriptor here is a
    /// file: a path, the executable, a memfd, or an unlinked file. The offset
    /// is any offset at all — the base-page rule binds where an image starts,
    /// not where the hydrator reads.
    ///
    /// The caller has already established that the descriptor holds
    /// `buf.len()` bytes from `at`; a short one is a truncated image and is
    /// reported as an I/O error rather than as a partial read.
    pub fn read_exact_at(&self, buf: &mut [u8], at: u64) -> Result<(), ImageError> {
        use std::os::unix::fs::FileExt;

        self.file.read_exact_at(buf, at)?;
        Ok(())
    }
}

/// The failure a bare libc call leaves behind. Only the memfd arm makes one:
/// the other reaches the kernel through `std`, which reports for itself.
#[cfg(any(target_os = "linux", target_os = "android"))]
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
    use std::os::fd::FromRawFd;
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
/// A host without `memfd_create` gets a file under `TMPDIR`, unlinked before
/// its first byte is written: the name is gone before there is anything to
/// read, so the descriptor is the only way back to the bytes and no failure
/// below can leave a file behind. It gets no seal, so immutability here is
/// this process's discipline rather than the kernel's.
///
/// POSIX shared memory is the obvious candidate and cannot serve. A Darwin
/// object of that kind takes `mmap` only with `MAP_SHARED`, and the hydrator
/// maps every page `MAP_FIXED | MAP_PRIVATE` (docs/impl/image.md).
///
/// Android takes the memfd arm above and never reaches this one.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn anonymous_file(bytes: &[u8]) -> Result<File, ImageError> {
    use std::os::unix::fs::FileExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    // The pid separates two processes and the counter separates two hydrations
    // inside one, so no two calls race on a name. `create_new` refuses an
    // existing entry rather than opening it, which is the check that holds if
    // both ever coincide anyway.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "elle-anon-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let file = File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)?;
    std::fs::remove_file(&path)?;
    file.write_all_at(bytes, 0)?;
    Ok(file)
}
