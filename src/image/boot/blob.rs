// audited: 2026-10-20
//! The embedded blob: the boot image a release executable carries, and where a
//! start finds its bytes.
//!
//! docs/impl/image/boot.md

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
        None
    }

    /// The blob's bytes.
    pub fn bytes(&self) -> &'static [u8] {
        self.bytes
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

#[cfg(test)]
#[repr(align(65536))]
pub(crate) struct Aligned<T: ?Sized>(pub(crate) T);

#[cfg(test)]
pub(crate) struct Located {
    pub(crate) file: std::fs::File,
    pub(crate) offset: u64,
}

#[cfg(test)]
pub(crate) fn locate(_bytes: &'static [u8]) -> Option<Located> {
    None
}

#[cfg(test)]
pub(crate) fn holds(_file: &std::fs::File, _offset: u64, _bytes: &[u8]) -> bool {
    true
}

#[cfg(test)]
impl Blob {
    pub(crate) fn open(&self) -> Result<super::OpenImage, crate::image::ImageError> {
        let source = crate::image::ImageSource::from_bytes(self.bytes)?;
        Ok(super::OpenImage::copied(source))
    }
}

#[cfg(test)]
mod tests;
