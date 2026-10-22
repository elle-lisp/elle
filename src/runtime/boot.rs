// audited: 2026-10-21
//! Where an instance boots from, and the boot that fills its core: a boot image it hydrates, or the three sources.
//!
//! docs/impl/image/boot.md
//! docs/impl/stdlib-cache.md
//!
//! The boot either hydrates an image or compiles the three sources, and either
//! way it records what the core's own workers boot from.

use std::sync::Arc;

use super::RuntimeCore;
use crate::compiler::stdlib_cache::StdlibCache;
use crate::image::boot::{BootImage, BootSource, OpenImage};
use crate::init_stdlib;
use crate::primitives::module_init::StdlibSource;
use crate::symbol::SymbolTable;

/// The two caches one instance boots through: where its compiled stdlib is
/// cached, and where its boot image is.
///
/// Both travel with the instance rather than coming from process state
/// (docs/impl/image/boot.md), so the pair is one construction parameter.
#[derive(Debug, Clone, Default)]
pub struct BootCaches {
    /// Where the compiled stdlib bytecode is cached
    /// (docs/impl/stdlib-cache.md). The instance reads it only when it hydrates
    /// no image and owes no store; its `sys/spawn` workers read it when it
    /// holds no image.
    pub stdlib: StdlibCache,
    /// Where the boot image is (docs/impl/image/boot.md). The process-wide
    /// policy by default: the embedded blob, then the warm cache.
    pub image: BootImage,
}

impl BootCaches {
    /// The process-wide choice of both: the `--cache=` stdlib directory, and
    /// the `--boot-image=` policy.
    pub fn process() -> Self {
        BootCaches {
            stdlib: StdlibCache::Process,
            image: crate::config::get().boot_image(),
        }
    }
}

/// What a worker spawned from this instance boots from: the image this
/// instance holds, and the stdlib cache a worker compiles through when it
/// holds none (docs/impl/image/boot.md).
///
/// The image is the descriptor this instance hydrated or stored, shared, so a
/// worker maps the bytes its parent mapped whatever happened to the path since.
#[derive(Debug, Clone, Default)]
pub struct WorkerBoot {
    image: Option<Arc<OpenImage>>,
    stdlib: StdlibCache,
}

/// Where an instance boots from.
pub(crate) enum BootFrom {
    /// A fresh instance's caches: the first image the policy names that
    /// hydrates is used, and a miss stores one where the policy names a file.
    Caches(BootCaches),
    /// A worker's parent: the image it holds is hydrated, and nothing is ever
    /// stored.
    Parent(WorkerBoot),
}

/// A booted core, and where its boot state came from.
pub(crate) struct Booted {
    pub(crate) core: RuntimeCore,
    pub(crate) stdlib_source: StdlibSource,
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
    /// or stored, and the stdlib cache `from` names.
    pub(crate) fn boot(gen: crate::segment::Generation, stdlib: bool, from: BootFrom) -> Booted {
        // A policy's image is the whole boot state, stdlib included, so an
        // instance that wants no stdlib reads and stores none — and
        // `--no-stdlib` is how core.lisp and the prelude are debugged.
        let (policy, images, cache) = match from {
            BootFrom::Caches(caches) if stdlib => {
                let images = caches.image.images().into_iter().map(Arc::new).collect();
                (Some(caches.image), images, caches.stdlib)
            }
            BootFrom::Caches(caches) => (None, Vec::new(), caches.stdlib),
            BootFrom::Parent(parent) => (None, parent.image.into_iter().collect(), parent.stdlib),
        };
        let (mut core, hydrated) = Self::for_boot(gen, &images);
        let (stdlib_source, boot_source, kept) = match hydrated {
            Some((boot, held)) => {
                let stdlib_source = if stdlib {
                    core.install_stdlib_from_image(&boot);
                    StdlibSource::Image
                } else {
                    StdlibSource::Compiled
                };
                (stdlib_source, BootSource::Image, Some(held))
            }
            None if stdlib => {
                // A boot image is dumped from a *compiled* stdlib. A disk-cache
                // hit rebuilds the library's closures through the send codec,
                // whose capture cells record no binding, and the dump refuses
                // one of those by variant (docs/impl/image/boot.md). So an
                // instance that owes an image compiles rather than reading the
                // other cache — once per build and digest, for the start that
                // stores it.
                let compiling = match policy.as_ref().and_then(BootImage::path) {
                    Some(_) => StdlibCache::Off,
                    None => cache.clone(),
                };
                let stdlib_source = core.load_stdlib(&compiling);
                // The boot is complete, so this is the state the next start
                // hydrates instead of repeating (docs/impl/image/boot.md).
                let stored = policy.and_then(|image| {
                    let t = std::time::Instant::now();
                    let (heap, symbols, cctx) = core.dump_parts();
                    let stored = crate::image::boot::store(heap, symbols, cctx, &image);
                    crate::phase!(crate::trace::boot(), "boot", t, "image-store");
                    stored
                });
                (stdlib_source, BootSource::Compiled, stored.map(Arc::new))
            }
            None => (StdlibSource::Compiled, BootSource::Compiled, None),
        };
        core.vm().set_worker_boot(WorkerBoot {
            image: kept,
            stdlib: cache,
        });
        Booted {
            core,
            stdlib_source,
            boot_source,
        }
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
    pub fn load_stdlib(&mut self, cache: &StdlibCache) -> StdlibSource {
        // Record it on the VM so a `sys/spawn` worker, which reaches the
        // spawning instance only through `ctx.vm()`, inherits this directory
        // instead of falling back to the process-wide one.
        self.vm().set_worker_boot(WorkerBoot {
            image: None,
            stdlib: cache.clone(),
        });
        let (vm, symbols, compile) = self.parts();
        init_stdlib(vm, symbols, compile, cache)
    }
}
