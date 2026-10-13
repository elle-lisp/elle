// audited: 2026-10-12
//! What a boot image carries that only this crate can see, and what the warm cache names its file.
//!
//! docs/impl/image/boot.md

mod registries;

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::compiler::stdlib_cache::StdlibCache;
use crate::runtime::{BootCaches, Runtime};

use super::cache::Executable;
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

    /// The image a source boot over [`Self::caches`] stored.
    fn image(&self) -> PathBuf {
        std::fs::read_dir(self.0.join("boot"))
            .expect("the boot directory exists")
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "image"))
            .expect("a stored image")
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
        .map(|(name, def)| (name.clone(), def.transformer().get().is_some()))
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
// (docs/impl/image/format.md): the installed expander's counter clears every
// scope the image's syntax carries, and an image boot mints from the same
// counter a source boot does.
//
// The trap is the size of that watermark. A boot graph's templates carry the
// prelude scope alone, which is zero, so the watermark is 1 — where a fresh
// expander already starts. The boot's own expansions mint on per-compile
// clones of the master expander, and a clone's counter never reaches the
// master, so the two boots agree here whether or not the raise runs at all.
// `raising_the_scope_counter_only_moves_it_up` below is the pin on the
// mechanism; this one pins the agreement.
#[test]
fn an_image_boot_mints_the_scopes_a_source_boot_would() {
    let scratch = Scratch::new("watermark");
    let counter = {
        let mut source = Runtime::with_caches(scratch.caches());
        assert_eq!(source.boot_source(), BootSource::Compiled);
        source.compile().scope_counter()
    };

    let mut rt = Runtime::with_caches(scratch.caches());
    assert_eq!(rt.boot_source(), BootSource::Image);
    assert_eq!(
        rt.compile().scope_counter(),
        counter,
        "an image boot mints from a different scope than the boot it was dumped from"
    );
    assert!(
        rt.compile().scope_counter() >= watermark_of(&scratch),
        "the expander mints inside the scopes the image's syntax carries"
    );
}

/// The scope watermark the image in `scratch` records, read out of the
/// artifact so the assertion above compares the instance against the file
/// rather than against itself.
fn watermark_of(scratch: &Scratch) -> u32 {
    let mut probe = Runtime::without_stdlib();
    let (heap, symbols) = probe.heap_and_symbols();
    super::hydrate_path(heap, symbols, &scratch.image())
        .expect("hydrate")
        .scope_watermark
}

// The mechanism the test above rests on, over a watermark no boot graph
// produces: a raise moves the counter and a lower one leaves it alone, so two
// images hydrated into one instance each raise and the highest wins.
#[test]
fn raising_the_scope_counter_only_moves_it_up() {
    let mut vm = crate::vm::VM::new();
    let mut expander = crate::syntax::Expander::on_vm(&mut vm);
    expander.raise_scope_counter(50);
    assert_eq!(expander.scope_counter(), 50);
    expander.raise_scope_counter(20);
    assert_eq!(expander.scope_counter(), 50, "a lower watermark lowered it");
}

/// Whether a call to the stdlib's `inc`, made by a REPL form with the JIT
/// eager, leaves compiled code for it in the cache.
#[cfg(feature = "jit")]
fn inc_promotes(rt: &mut Runtime) -> bool {
    rt.vm().runtime_config.jit = crate::config::JitPolicy::Eager;
    let inc = {
        let (vm, symbols, cctx) = rt.parts();
        crate::pipeline::eval_all("inc", symbols, vm, cctx, "<tier>").expect("inc")
    };
    let (bytecode, signal, arity) = {
        let t = &inc.as_closure().expect("inc is a closure").template;
        (t.bytecode().as_ptr(), t.signal(), t.arity())
    };
    // A REPL binding the compiler knows only by value, so the call below is a
    // call: a name the inline registry carries would have its body spliced in.
    let sym = rt.symbols().intern("tier-f");
    {
        let (cctx, heap) = rt.compile_and_heap();
        cctx.register_repl_binding(
            heap,
            sym,
            inc,
            crate::value::arena::RootRef::Mint,
            signal,
            Some(arity),
        );
    }
    let (vm, symbols, cctx) = rt.parts();
    crate::pipeline::eval_all("(tier-f 1)", symbols, vm, cctx, "<tier>").expect("call");
    vm.drain_jit_pending();
    vm.jit_code_for(bytecode).is_some()
}

// § Test plan, "Tier parity" (docs/impl/image/plan.md): a hot stdlib function
// reaches the JIT under an image boot exactly as under a source boot, because
// the hydrated payload carries its LIR.
//
// The counter-factual is a hydrated header with no LIR: every call still
// answers, on the interpreter, and only the cache can tell. The source boot
// runs first, so a stdlib function the JIT refuses fails here rather than
// passing both boots vacuously.
#[cfg(feature = "jit")]
#[test]
fn a_hot_stdlib_function_compiles_on_the_jit_under_either_boot() {
    let scratch = Scratch::new("tier");
    let mut source = Runtime::with_caches(scratch.caches());
    assert_eq!(source.boot_source(), BootSource::Compiled);
    assert!(
        inc_promotes(&mut source),
        "a source boot does not compile inc, so this test exercises nothing"
    );
    drop(source);

    let mut hydrated = Runtime::with_caches(scratch.caches());
    assert_eq!(hydrated.boot_source(), BootSource::Image);
    assert!(
        inc_promotes(&mut hydrated),
        "an image boot leaves inc on the interpreter tier"
    );
}

// ── The warm cache's file name ───────────────────────────────────────

// § "The warm cache": one executable rebuilt gives one source digest two
// names. The counter-factual is a name from the digest alone: the new build
// opens the image the old one stored, and its registries decode as whatever
// the old build's HIR meant.
#[test]
fn two_builds_name_one_digests_image_apart() {
    let old = Executable::new(12_563_264, 1);
    let new = Executable::new(12_563_264, 2);
    let digest = super::sources_digest();
    assert_ne!(
        old.image_name(&digest),
        new.image_name(&digest),
        "two builds file one digest's image under one name"
    );
}

// § "The warm cache": a binary that cannot read its own identity reads and
// writes no image, as the stdlib cache declines to cache. Falling back to the
// digest alone would bring back the confusion the identity exists to prevent.
#[test]
fn a_binary_with_no_identity_has_no_image_path() {
    let policy = BootImage::Dir(PathBuf::from("/nowhere/boot"));
    assert_eq!(policy.path_for(None), None);
}

/// The identity is read from the executable this process runs, which is the
/// file whose rebuild must change the key.
///
/// Characterization, not a failing-first regression: "a different build
/// yields a different key" is observable only across builds.
#[test]
fn the_build_identity_is_read_from_the_running_executable() {
    let exe = Executable::current().expect("the test binary can identify itself");
    let (len, _mtime) = exe.identity();
    let meta = std::fs::metadata(std::env::current_exe().expect("current_exe"))
        .expect("the executable's metadata");
    assert_eq!(
        len,
        meta.len(),
        "the identity must be the binary's own size"
    );
    assert!(len > 0, "a zero-length identity separates nothing");
}
