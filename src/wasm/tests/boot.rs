// audited: 2026-11-25
//! The host runtime a full-module run boots, and the boot image it hydrates.
//!
//! docs/impl/image/boot.md
//! docs/impl/wasm.md

use crate::image::boot::{BootImage, BootSource};

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
    let image = || BootImage::Dir(dir.path().to_path_buf());
    assert_eq!(
        super::super::host_boot_source(true, image()),
        BootSource::Compiled,
        "the first boot found an image in an empty directory"
    );
    assert_eq!(
        super::super::host_boot_source(true, image()),
        BootSource::Image,
        "the second boot did not hydrate the image the first stored"
    );
}
