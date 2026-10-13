// audited: 2026-10-20
//! The embedded blob: the boot image a release executable carries, and where a
//! start finds its bytes.
//!
//! docs/impl/image/boot.md
//!
//! A start maps the blob from the file that holds it, at the offset the
//! segment table gives, when the bytes there begin with the blob's header
//! block. Otherwise it copies the blob into an anonymous file and hydrates
//! that.

use std::fs::File;
use std::path::PathBuf;

use crate::image::{ImageError, ImageSource};

use super::OpenImage;

/// Bytes at the largest base page any supported host reports, so a blob's file
/// offset is a multiple of every host's page (docs/impl/image/boot.md).
#[repr(align(65536))]
pub(crate) struct Aligned<T: ?Sized>(pub(crate) T);

/// The blob a release build compiles in. A build that names none carries no
/// bytes, which [`Blob::this_binary`] reads as no blob.
#[cfg(elle_boot_blob)]
static EMBEDDED: &Aligned<[u8]> = &Aligned(*include_bytes!(env!("ELLE_BOOT_BLOB_PATH")));
#[cfg(not(elle_boot_blob))]
static EMBEDDED: &Aligned<[u8]> = &Aligned([0u8; 0]);

/// A boot image held in this process's memory: the blob a release build
/// compiles in, or bytes a host hands over.
///
/// The bytes are `'static` because a start maps them from the file that holds
/// them, and that file outlives every instance.
#[derive(Clone, Copy)]
pub struct Blob {
    bytes: &'static [u8],
}

impl Blob {
    /// A blob over `bytes`.
    pub fn new(bytes: &'static [u8]) -> Blob {
        Blob { bytes }
    }

    /// The blob this binary carries, or `None` for a build that embeds none.
    pub fn this_binary() -> Option<Blob> {
        let bytes: &'static [u8] = &EMBEDDED.0;
        (!bytes.is_empty()).then(|| Blob::new(bytes))
    }

    /// The blob's bytes.
    pub fn bytes(&self) -> &'static [u8] {
        self.bytes
    }

    /// The blob, held open for a hydration: mapped from the file that holds
    /// it, or copied into an anonymous file when no file holds it here.
    pub(crate) fn open(&self) -> Result<OpenImage, ImageError> {
        let t = std::time::Instant::now();
        if let Some(open) = self.mapped() {
            crate::phase!(crate::trace::boot(), "boot", t, "blob-map");
            return Ok(open);
        }
        let source = ImageSource::from_bytes(self.bytes)?;
        crate::phase!(crate::trace::boot(), "boot", t, "blob-copy");
        Ok(OpenImage::copied(source))
    }

    /// The blob in the file that holds it, or `None` when no file holds its
    /// bytes at a base-page offset.
    fn mapped(&self) -> Option<OpenImage> {
        let Located { file, path, offset } = locate(self.bytes)?;
        if !holds(&file, offset, self.bytes) {
            return None;
        }
        let source = ImageSource::at(file, offset).ok()?;
        Some(OpenImage::embedded(source, path))
    }
}

/// Two blobs are one blob when they are the same bytes in memory.
impl PartialEq for Blob {
    fn eq(&self, other: &Blob) -> bool {
        std::ptr::eq(self.bytes, other.bytes)
    }
}

impl Eq for Blob {}

/// The address and the length; the bytes are megabytes.
impl std::fmt::Debug for Blob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Blob")
            .field("at", &self.bytes.as_ptr())
            .field("len", &self.bytes.len())
            .finish()
    }
}

/// Where bytes in memory sit in a file: the file, open, the path a report
/// names, and the offset of the first byte.
pub(crate) struct Located {
    pub(crate) file: File,
    pub(crate) path: PathBuf,
    pub(crate) offset: u64,
}

/// The file that holds `bytes`, or `None` when they lie in no file-backed
/// segment of any loaded object, or the file does not open.
pub(crate) fn locate(bytes: &'static [u8]) -> Option<Located> {
    let (path, offset) = segment_of(bytes.as_ptr() as usize, bytes.len())?;
    let file = File::open(&path).ok()?;
    Some(Located { file, path, offset })
}

/// Whether `file` holds `bytes` at `offset`: the bytes there begin with the
/// blob's header block, or are the whole of a blob shorter than one.
///
/// The header block carries the fingerprint and every section's size, so a
/// file another build wrote differs there. Comparing the block alone leaves
/// the rest of the blob unread, which keeps its pages out of memory until a
/// hydration faults them in.
pub(crate) fn holds(file: &File, offset: u64, bytes: &[u8]) -> bool {
    use std::os::unix::fs::FileExt;

    let want = &bytes[..bytes.len().min(crate::image::format::HEADER_BLOCK)];
    let mut found = vec![0u8; want.len()];
    file.read_exact_at(&mut found, offset).is_ok() && found == want
}

/// The file that maps `len` bytes at `addr`, and the offset of `addr` in it.
///
/// The loader's segment table answers: the segment whose file-backed part
/// holds the range, and that segment's file offset. The main program is
/// opened through `/proc/self/exe`, which names the running file even after
/// a rebuild replaced its path.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn segment_of(addr: usize, len: usize) -> Option<(PathBuf, u64)> {
    use std::ffi::{c_int, c_void, CStr, OsStr};
    use std::os::unix::ffi::OsStrExt;

    struct Search {
        addr: usize,
        len: usize,
        visited: usize,
        found: Option<(PathBuf, u64)>,
    }

    // SAFETY (for the loader): `data` is the `Search` the call below passes,
    // and `info` is valid for the duration of the callback.
    unsafe extern "C" fn visit(
        info: *mut libc::dl_phdr_info,
        _size: libc::size_t,
        data: *mut c_void,
    ) -> c_int {
        let search = unsafe { &mut *(data as *mut Search) };
        let info = unsafe { &*info };
        // The loader visits the main program first.
        let main = search.visited == 0;
        search.visited += 1;
        if info.dlpi_phdr.is_null() {
            return 0;
        }
        let base = info.dlpi_addr as usize;
        let phdrs = unsafe { std::slice::from_raw_parts(info.dlpi_phdr, info.dlpi_phnum as usize) };
        for ph in phdrs.iter().filter(|ph| ph.p_type == libc::PT_LOAD) {
            let start = base.wrapping_add(ph.p_vaddr as usize);
            let backed = start.wrapping_add(ph.p_filesz as usize);
            if search.addr < start || search.addr + search.len > backed {
                continue;
            }
            // A 32-bit ELF target's segment offset is a `u32`.
            #[allow(clippy::unnecessary_cast)]
            let offset = ph.p_offset as u64 + (search.addr - start) as u64;
            let named = if info.dlpi_name.is_null() {
                &[][..]
            } else {
                unsafe { CStr::from_ptr(info.dlpi_name) }.to_bytes()
            };
            let path = if main || named.is_empty() {
                PathBuf::from("/proc/self/exe")
            } else {
                PathBuf::from(OsStr::from_bytes(named))
            };
            search.found = Some((path, offset));
            return 1;
        }
        0
    }

    let mut search = Search {
        addr,
        len,
        visited: 0,
        found: None,
    };
    // SAFETY: `visit` reads only what the loader hands it and the `Search`
    // this frame owns, which outlives the call.
    unsafe { libc::dl_iterate_phdr(Some(visit), &mut search as *mut Search as *mut c_void) };
    search.found
}

#[cfg(target_os = "macos")]
mod macho;
#[cfg(target_os = "macos")]
use macho::segment_of;

/// No other host has a segment table this reads, so a blob is always copied
/// there.
#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
fn segment_of(_addr: usize, _len: usize) -> Option<(PathBuf, u64)> {
    None
}

#[cfg(test)]
mod tests;
