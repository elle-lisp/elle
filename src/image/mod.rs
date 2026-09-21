// audited: 2026-09-21
//! Image persistence: an image is the page bytes of one compacted region plus
//! a relocation table, and hydration maps those pages privately.
//!
//! docs/impl/image.md
//! docs/impl/image/format.md
//!
//! Hydration rewrites the pointer slots, replays the name, file and primitive
//! tables into the hydrating instance, runs the reconstruction stream, raises
//! the watermarked counters, and installs the result as an ordinary counted
//! region. The body carries the whole sealed data set: pairs, strings, bytes,
//! arrays, sets, structs, syntax, floats, parameters, user trait tables,
//! closures and their code objects, and the portable immediates — symbols,
//! keywords and native-fns among them. `boot` is the configuration that dumps
//! a booted instance and hydrates one; the environment configuration arrives
//! with a later milestone (docs/impl/image/plan.md).

pub mod boot;
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
    /// One past the highest hygiene scope counter the body carries. An
    /// expander that will meet this image's syntax mints above it, or two
    /// unrelated scopes compare equal (docs/impl/image/format.md). Zero when
    /// the body holds no syntax.
    pub scope_watermark: u32,
    /// One past the highest parameter id the body carries; zero when the body
    /// holds no parameter. Hydration has already raised this process's counter
    /// past it — the field is what a caller reads to see that it did.
    pub param_watermark: u32,
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
    /// The image was built from different boot sources. Its layout is one this
    /// binary can map and its library is one this binary must not answer with,
    /// so the digest refuses it where the fingerprint cannot
    /// (docs/impl/image/boot.md).
    Sources {
        expected: String,
        found: String,
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
            ImageError::Sources { expected, found } => write!(
                f,
                "image was built from different boot sources: digest {found}, this binary is {expected}"
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
