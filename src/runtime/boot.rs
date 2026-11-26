// audited: 2026-11-26
//! Where an instance boots from, and the boot that fills its core: a boot image it hydrates, or the three sources.
//!
//! docs/impl/image/boot.md
//!
//! The boot either hydrates an image or compiles the three sources, and either
//! way it records what the core's own workers boot from.

use std::sync::Arc;

use super::RuntimeCore;
use crate::image::boot::{BootImage, BootSource, OpenImage};
use crate::init_stdlib;
use crate::symbol::SymbolTable;

/// What a worker spawned from this instance boots from: the image this
/// instance holds. A worker whose parent holds none compiles the three sources
/// (docs/impl/image/boot.md).
///
/// The image is the descriptor this instance hydrated or stored, shared, so a
/// worker maps the bytes its parent mapped whatever happened to the path since.
#[derive(Debug, Clone, Default)]
pub struct WorkerBoot {
    image: Option<Arc<OpenImage>>,
}

/// Where an instance boots from.
pub(crate) enum BootFrom {
    /// A fresh instance's image policy: the first image the policy names that
    /// hydrates is used, and a miss stores one where the policy names a file.
    Policy(BootImage),
    /// A worker's parent: the image it holds is hydrated, and nothing is ever
    /// stored.
    Parent(WorkerBoot),
}

/// A booted core, and where its boot state came from.
pub(crate) struct Booted {
    pub(crate) core: RuntimeCore,
    pub(crate) boot_source: BootSource,
}

/// Hydrate the first of `images` this binary accepts, or answer `None` and
/// leave the instance to compile its three sources.
///
/// Each refusal is reported and swallowed, because a boot image is a speedup.
/// Every reason to refuse one — a fingerprint from another build, a digest from
/// other sources, a truncated file — is a reason to try the next or compile.
/// The report is what keeps a cache that never hits from looking like one that
/// always does. The answer names the image that hydrated, which the instance
/// hands its workers.
pub(super) fn hydrate_boot_image(
    heap: &mut crate::value::fiberheap::FiberHeap,
    symbols: &mut SymbolTable,
    images: &[Arc<OpenImage>],
) -> Option<(crate::image::boot::Boot, Arc<OpenImage>)> {
    for image in images {
        let t = std::time::Instant::now();
        match crate::image::boot::hydrate(heap, symbols, image.source()) {
            Ok(boot) => {
                crate::phase!(crate::trace::boot(), "boot", t, "image-hydrate");
                return Some((boot, Arc::clone(image)));
            }
            Err(e) => eprintln!("[boot-image] {}: {e}", image.origin()),
        }
    }
    None
}

impl RuntimeCore {
    /// Boot a core from `from`: out of a boot image where there is one this
    /// binary hydrates, out of the three sources otherwise
    /// (docs/impl/image/boot.md). With `stdlib` false the core holds core.lisp
    /// and the prelude alone.
    ///
    /// The core records what its own workers boot from: the image it hydrated
    /// or stored.
    pub(crate) fn boot(gen: crate::segment::Generation, stdlib: bool, from: BootFrom) -> Booted {
        // A policy's image is the whole boot state, stdlib included, so an
        // instance that wants no stdlib reads and stores none — and
        // `--no-stdlib` is how core.lisp and the prelude are debugged.
        let (policy, images) = match from {
            BootFrom::Policy(image) if stdlib => {
                let images = image.images().into_iter().map(Arc::new).collect();
                (Some(image), images)
            }
            BootFrom::Policy(_) => (None, Vec::new()),
            BootFrom::Parent(parent) => (None, parent.image.into_iter().collect()),
        };
        let (mut core, hydrated) = Self::for_boot(gen, &images);
        let (boot_source, kept) = match hydrated {
            Some((boot, held)) => {
                if stdlib {
                    core.install_stdlib_from_image(&boot);
                }
                (BootSource::Image, Some(held))
            }
            None if stdlib => {
                core.load_stdlib();
                // The boot is complete, so this is the state the next start
                // hydrates instead of repeating (docs/impl/image/boot.md).
                let stored = policy.and_then(|image| {
                    let t = std::time::Instant::now();
                    let (heap, symbols, cctx) = core.dump_parts();
                    let stored = crate::image::boot::store(heap, symbols, cctx, &image);
                    crate::phase!(crate::trace::boot(), "boot", t, "image-store");
                    stored
                });
                (BootSource::Compiled, stored.map(Arc::new))
            }
            None => (BootSource::Compiled, None),
        };
        core.vm().set_worker_boot(WorkerBoot { image: kept });
        Booted { core, boot_source }
    }

    /// Install a hydrated image's stdlib exports, in place of compiling and
    /// running stdlib.lisp. The image's region is already a process root, so
    /// this roots nothing of its own (docs/impl/image/boot.md).
    fn install_stdlib_from_image(&mut self, boot: &crate::image::boot::Boot) {
        let exports = boot.stdlib_exports();
        let (_vm, symbols, compile) = self.parts();
        crate::primitives::module_init::install_exports(symbols, compile, exports);
    }

    /// Compile and execute stdlib.lisp into this core's `CompileCtx`. The caller
    /// must have installed the symbol-table context first (stdlib macros gensym).
    pub fn load_stdlib(&mut self) {
        let (vm, symbols, compile) = self.parts();
        init_stdlib(vm, symbols, compile);
    }
}
