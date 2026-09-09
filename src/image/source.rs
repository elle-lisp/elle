// audited: 2026-09-08
//! Where an image's bytes live: a descriptor, and the offset of the image's
//! first byte inside it.
//!
//! docs/impl/image/format.md

use std::fs::File;
use std::os::fd::{AsFd, BorrowedFd};
use std::path::Path;

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
    pub fn open(_path: &Path) -> Result<ImageSource, ImageError> {
        Err(ImageError::Corrupt(
            "ImageSource::open is not built yet".into(),
        ))
    }

    /// An image starting at `offset` inside an already-open file.
    ///
    /// The offset must be a multiple of the OS base page: every page maps at
    /// this offset plus a base-page multiple, and `mmap` accepts no other
    /// file offset.
    pub fn at(_file: File, _offset: u64) -> Result<ImageSource, ImageError> {
        Err(ImageError::Corrupt(
            "ImageSource::at is not built yet".into(),
        ))
    }

    /// An image that arrived as bytes, in an anonymous memory file that no
    /// filesystem names.
    pub fn from_bytes(_bytes: &[u8]) -> Result<ImageSource, ImageError> {
        Err(ImageError::Corrupt(
            "ImageSource::from_bytes is not built yet".into(),
        ))
    }

    /// The descriptor to map from.
    pub fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }

    /// The image's first byte, as an offset into that descriptor.
    pub fn offset(&self) -> u64 {
        self.offset
    }
}
