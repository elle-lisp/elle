// audited: 2026-10-13
//! The host runtime a full-module run boots, and the boot image it hydrates.
//!
//! docs/impl/image/boot.md
//! docs/impl/wasm.md

use crate::compiler::stdlib_cache::StdlibCache;
use crate::image::boot::{BootImage, BootSource};
use crate::runtime::BootCaches;

/// The host runtime boots as `elle` does: over an empty directory it compiles
/// and stores, and the next boot hydrates what it stored. The counter-factual
/// is a host that loads the stdlib through `init_stdlib`, which compiles at
/// every boot and reports no image.
///
/// The trap: a process that declared a user signal refuses every boot dump, so
/// this pin holds only in a run that declares none, as the `wasm::` filter is.
#[test]
fn the_host_runtime_hydrates_the_image_a_host_runtime_stored() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let caches = || BootCaches {
        stdlib: StdlibCache::Off,
        image: BootImage::Dir(dir.path().to_path_buf()),
    };
    assert_eq!(
        super::super::host_boot_source(true, caches()),
        BootSource::Compiled,
        "the first boot found an image in an empty directory"
    );
    assert_eq!(
        super::super::host_boot_source(true, caches()),
        BootSource::Image,
        "the second boot did not hydrate the image the first stored"
    );
}
