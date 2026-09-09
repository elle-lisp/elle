// audited: 2026-09-08
//! Image persistence: an image is the page bytes of one compacted region plus
//! a relocation table, and hydration maps those pages privately.
//!
//! docs/impl/image.md
//! docs/impl/image/format.md
//!
//! Hydration rewrites the pointer slots, replays the name table into the
//! hydrating instance's display memo, and installs the result as an ordinary
//! counted region. Current scope is the store milestone's data-only set:
//! pairs, strings, bytes, arrays, floats, and the portable immediates —
//! symbols and keywords among them — dumped and hydrated end to end.
//! Closures, structs, sets, syntax, and the boot and environment
//! configurations arrive with the later milestones
//! (docs/impl/image/plan.md).

mod dump;
mod format;
mod hydrate;
mod layout;
mod source;
mod verify;

pub use dump::dump;
pub use format::{fingerprint, sections, Sections};
pub use hydrate::{hydrate, hydrate_path};
pub use source::ImageSource;

use crate::hir::region::RuntimeRegion;
use crate::value::Value;

/// A successful hydration: the image's root value and the counted region
/// holding the mapped pages (rc 1 — the caller's reference; release it with
/// the ordinary region RC, or register it as a process root).
#[derive(Debug, Clone, Copy)]
pub struct Hydrated {
    pub root: Value,
    pub region: RuntimeRegion,
}

/// Why a dump or hydration refused.
#[derive(Debug)]
pub enum ImageError {
    Io(std::io::Error),
    /// The image was built by a binary whose layout disagrees with this one.
    /// Fall back to sources — images are regenerated, never migrated.
    Fingerprint {
        expected: String,
        found: String,
    },
    /// The image does not start on a base-page boundary of its descriptor, so
    /// no page of it can be mapped. Whoever placed the image chose the
    /// offset; the same descriptor is legal once the image moves.
    Unaligned {
        offset: u64,
        page: usize,
    },
    /// The graph holds a value the dump policy refuses, named.
    Unsupported(String),
    /// The file is not a well-formed image for this format version.
    Corrupt(String),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::Io(e) => write!(f, "image io: {e}"),
            ImageError::Fingerprint { expected, found } => {
                write!(
                    f,
                    "image fingerprint mismatch: built for {found:?}, this binary is {expected:?}"
                )
            }
            ImageError::Unaligned { offset, page } => write!(
                f,
                "image offset {offset} is not a multiple of the {page}-byte page size"
            ),
            ImageError::Unsupported(what) => write!(f, "image refuses: {what}"),
            ImageError::Corrupt(what) => write!(f, "corrupt image: {what}"),
        }
    }
}

impl std::error::Error for ImageError {}

impl From<std::io::Error> for ImageError {
    fn from(e: std::io::Error) -> Self {
        ImageError::Io(e)
    }
}
