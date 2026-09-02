//! Image persistence, store milestone (docs/impl/image.md — the design
//! home). An image is the page bytes of one compacted region plus a
//! relocation table; hydration maps the pages privately, rewrites the
//! pointer slots, and installs the result as an ordinary counted region.
//!
//! Current scope is the data-only spike (§ "Landing order" item 5): sealed
//! data graphs — pairs, strings, bytes, arrays, floats, portable immediates
//! — dumped and hydrated end to end. Closures, structs, sets, symbols, and
//! the boot/environment configurations arrive with the foundations and the
//! later milestones.

mod dump;
mod format;
mod hydrate;
mod layout;

pub use dump::dump;
pub use format::fingerprint;
pub use hydrate::hydrate;

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
