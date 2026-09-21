// audited: 2026-09-21
//! Booting from an image: what a boot dump carries, what a fresh instance
//! answers out of it, and what the warm cache stores.
//!
//! docs/impl/image/boot.md
//! docs/impl/image/plan.md
//!
//! This is its own test binary because a boot dump is refused by any signal
//! the process declared. Signal inference marks a core.lisp closure as able to
//! signal anything, so its mask names every user bit the registry has handed
//! out — and one `(signal :kw)` anywhere in a shared binary refuses every boot
//! dump beside it. A boot dump happens before any program runs, so the
//! condition is a test-binary one rather than a limit on the feature.

#[path = "common/mod.rs"]
mod common;

use common::paint_stack;
use elle::image::ImageError;
use elle::runtime::Runtime;
use elle::value::Value;

mod boot {
    include!("image_boot/boot.rs");
}
mod warm {
    include!("image_boot/warm.rs");
}
