// audited: 2026-09-21
//! What a boot image carries that nothing outside this crate can see: the
//! transformer caches, and the expander's scope counter.
//!
//! docs/impl/image/boot.md

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::compiler::stdlib_cache::StdlibCache;
use crate::runtime::{BootCaches, Runtime};

use super::{BootImage, BootSource};

/// A scratch directory this test owns, removed when it goes out of scope.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("elle-boot-{}-{}", tag, std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(dir)
    }

    fn caches(&self) -> BootCaches {
        BootCaches {
            stdlib: StdlibCache::Dir(self.0.join("stdlib")),
            image: BootImage::Dir(self.0.join("boot")),
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Each macro this instance holds, with whether its transformer cache is
/// filled. The pair is the whole of what the image has to carry: a name with
/// no entry is a macro the hydrated instance cannot expand, and a filled cache
/// that arrives empty is a transformer compiled in some later expansion
/// context instead of the real one.
fn macro_shape(rt: &mut Runtime) -> BTreeSet<(String, bool)> {
    rt.compile()
        .macros()
        .iter()
        .map(|(name, def)| (name.clone(), def.cached_transformer.borrow().is_some()))
        .collect()
}

// § "Macros persist whole" (docs/impl/image/sealing.md): the filled caches
// hydrate without recompiling, and a cache the boot never filled stays empty
// and fills lazily as today. So the table an image boot installs carries the
// same names with the same split as the source boot it was dumped from.
//
// The counter-factual is a dump that drops the caches: every name still
// arrives, every macro still expands, and each one silently recompiles its
// transformer in the first expansion context that asks — which is the
// mis-scoping the lazy fill exists to avoid.
#[test]
fn the_macro_table_crosses_with_its_filled_transformers() {
    let scratch = Scratch::new("macros");
    let mut source = Runtime::with_caches(scratch.caches());
    assert_eq!(source.boot_source(), BootSource::Compiled);
    let before = macro_shape(&mut source);
    assert!(
        before.iter().any(|(_, filled)| *filled),
        "no transformer was filled at boot, so this test exercises nothing"
    );
    assert!(
        before.iter().any(|(_, filled)| !*filled),
        "every transformer was filled at boot, so the lazy half is untested"
    );
    drop(source);

    let mut hydrated = Runtime::with_caches(scratch.caches());
    assert_eq!(hydrated.boot_source(), BootSource::Image);
    assert_eq!(
        macro_shape(&mut hydrated),
        before,
        "the hydrated macro table differs from the one that was dumped"
    );
}

// § "The watermarks bound what a fresh instance may mint"
// (docs/impl/image/format.md): an expander starts its counter at one, so a
// hydrated template's scopes and a fresh expander's would collide. The
// installed counter must clear every scope the image's templates carry.
//
// The counter-factual is an expander left at its own start: two unrelated
// scopes compare equal, and a template symbol can resolve to a user binding
// that happens to share the number.
#[test]
fn a_fresh_expander_mints_scopes_above_the_images_watermark() {
    let scratch = Scratch::new("watermark");
    drop(Runtime::with_caches(scratch.caches()));

    // Read the watermark out of the image directly, so the assertion compares
    // the instance against the artifact rather than against itself.
    let path = std::fs::read_dir(scratch.0.join("boot"))
        .expect("the boot directory exists")
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "image"))
        .expect("the first boot stored an image");
    let watermark = {
        let mut probe = Runtime::without_stdlib();
        let (heap, symbols) = probe.heap_and_symbols();
        super::hydrate_path(heap, symbols, &path)
            .expect("hydrate")
            .scope_watermark
    };
    assert!(watermark > 1, "the image carries no scopes: {watermark}");

    let mut rt = Runtime::with_caches(scratch.caches());
    assert_eq!(rt.boot_source(), BootSource::Image);
    assert!(
        rt.compile().scope_counter() >= watermark,
        "the expander mints from {}, inside the image's scopes (watermark {watermark})",
        rt.compile().scope_counter()
    );
}
