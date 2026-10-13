// audited: 2026-10-20
//! A boot image held open: the descriptor a hydration maps, and where it came
//! from, which a refusal names.
//!
//! docs/impl/image/boot.md

use std::path::PathBuf;

use crate::image::ImageSource;

/// A boot image held open, for this instance's hydration and for every worker
/// it spawns.
///
/// A worker hydrates from its parent's, so the descriptor travels rather than
/// a path (docs/impl/image/boot.md). A prune or a rename over the path after
/// this opened it changes nothing a hydration maps.
#[derive(Debug)]
pub(crate) struct OpenImage {
    source: ImageSource,
    origin: Origin,
}

/// Where an open image came from.
#[derive(Debug)]
pub(crate) enum Origin {
    /// A warm-cache file, opened at this path.
    File(PathBuf),
    /// The embedded blob, in the file at this path.
    Embedded(PathBuf),
    /// The embedded blob, copied into an anonymous file.
    Copied,
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Origin::File(path) => write!(f, "{}", path.display()),
            Origin::Embedded(path) => write!(f, "the blob in {}", path.display()),
            Origin::Copied => write!(f, "the blob, copied"),
        }
    }
}

impl OpenImage {
    /// A warm-cache file, open, and the path it was opened at.
    pub(crate) fn file(source: ImageSource, path: PathBuf) -> OpenImage {
        OpenImage {
            source,
            origin: Origin::File(path),
        }
    }

    /// The embedded blob, in the file at `path` that holds it.
    pub(crate) fn embedded(source: ImageSource, path: PathBuf) -> OpenImage {
        OpenImage {
            source,
            origin: Origin::Embedded(path),
        }
    }

    /// The embedded blob, copied into the anonymous file `source` reads.
    pub(crate) fn copied(source: ImageSource) -> OpenImage {
        OpenImage {
            source,
            origin: Origin::Copied,
        }
    }

    /// The descriptor and offset a hydration maps.
    pub(crate) fn source(&self) -> &ImageSource {
        &self.source
    }

    /// Where the image came from.
    pub(crate) fn origin(&self) -> &Origin {
        &self.origin
    }
}
