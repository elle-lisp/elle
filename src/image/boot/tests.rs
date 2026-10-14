// audited: 2026-10-13
//! What a boot image carries that only this crate can see: transformer caches, the names a unit mints, and the stdlib's JIT tier.
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

// § "Booting from one" (docs/impl/image/boot.md): the installed expander mints the
// scopes a source boot would, and gensym answers the names it would, because
// both name the unit that mints them. The boot raises no counter for it.
//
// The counter-factual is a count the instance carries from its boot: the
// source boot and the image boot reach a user's file having done different
// work, and the file's names would differ between them.
#[test]
fn an_image_boot_expands_a_unit_as_a_source_boot_does() {
    use crate::syntax::tests::units::{run_file, FILE_UNIT};

    let scratch = Scratch::new("units");
    let minted = {
        let mut source = Runtime::with_caches(scratch.caches());
        assert_eq!(source.boot_source(), BootSource::Compiled);
        run_file(&mut source, FILE_UNIT, "unit.lisp")
    };

    let mut rt = Runtime::with_caches(scratch.caches());
    assert_eq!(rt.boot_source(), BootSource::Image);
    assert_eq!(
        run_file(&mut rt, FILE_UNIT, "unit.lisp"),
        minted,
        "an image boot expands a unit differently from the boot it was dumped from"
    );
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
    // A host binding the compiler knows only by value, so the call below is a
    // call: a name the inline registry carries would have its body spliced in.
    let sym = rt.symbols().intern("tier-f");
    {
        let (cctx, heap) = rt.compile_and_heap();
        cctx.register_host_binding(
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
