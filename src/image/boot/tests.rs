// audited: 2026-10-21
//! What a boot image carries that only this crate can see, and compile parity under either boot.
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
/// filled. The pair is the whole of what the image has to carry. A name with no
/// entry is a macro the hydrated instance cannot expand. A filled cache that
/// arrives empty is a transformer compiled in some later expansion context
/// instead of the real one.
fn macro_shape(rt: &mut Runtime) -> BTreeSet<(String, bool)> {
    rt.compile()
        .macros()
        .iter()
        .map(|(name, def)| (name.clone(), def.transformer().get().is_some()))
        .collect()
}

// docs/impl/image/sealing.md: the filled caches hydrate without recompiling,
// and a cache the boot never filled stays empty and fills lazily as today. So
// the table an image boot installs carries the same names with the same split
// as the source boot it was dumped from.
//
// The counter-factual is a dump that drops the caches. Every name still
// arrives and every macro still expands. But each one silently recompiles its
// transformer in the first expansion context that asks, which is the
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

// docs/impl/image/boot.md: the installed expander mints the scopes a source
// boot would, and gensym answers the names it would, because both name the
// unit that mints them. The boot raises no counter for it.
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
    // The graft would replace the call below with inc's body, which it reads
    // off the value, and the question here is whether inc's own code reaches
    // the JIT. So this compile keeps the call a call.
    rt.compile().set_graft(crate::config::GraftPolicy::Off);
    let inc = {
        let (vm, symbols, cctx) = rt.parts();
        crate::pipeline::eval_all("inc", symbols, vm, cctx, "<tier>").expect("inc")
    };
    let (bytecode, signal, arity) = {
        let t = &inc.as_closure().expect("inc is a closure").template;
        (t.bytecode().as_ptr(), t.signal(), t.arity())
    };
    // A host binding, so the call below reaches inc's closure through the
    // ordinary dispatch path.
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

// docs/impl/image/plan.md: a hot stdlib function reaches the JIT under an
// image boot exactly as under a source boot, because the hydrated payload
// carries its LIR.
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

/// A user file whose one call to `+` grafts under a source boot.
const GRAFTS_PLUS: &str = "(defn h [a b] (+ a b)) (h 1 2)";

/// The calls to `+` left in `src` once `rt` has compiled it.
fn plus_calls(rt: &mut Runtime, src: &str) -> usize {
    let (_vm, symbols, cctx) = rt.parts();
    let (hir, arena) = crate::pipeline::compile_file_to_fhir(src, symbols, cctx, "<graft>")
        .expect("the probe compiles");
    crate::hir::testkit::count_calls_to(&hir, &arena, "+")
}

// docs/impl/image/pins.md: a call to a stdlib function grafts under an image
// boot exactly as under a source boot, because the hydrated payload carries
// the body fragment the graft reads.
//
// The counter-factual is a hydrated payload with no fragment: the call stays
// a call, answers correctly, and only the tree can tell. The source boot runs
// first, so a file nothing grafts fails here rather than passing both boots
// vacuously.
#[test]
fn a_call_to_a_stdlib_function_grafts_under_either_boot() {
    let scratch = Scratch::new("graft");
    let mut source = Runtime::with_caches(scratch.caches());
    assert_eq!(source.boot_source(), BootSource::Compiled);
    assert_eq!(
        plus_calls(&mut source, GRAFTS_PLUS),
        0,
        "a source boot does not graft +, so this test exercises nothing"
    );
    drop(source);

    let mut hydrated = Runtime::with_caches(scratch.caches());
    assert_eq!(hydrated.boot_source(), BootSource::Image);
    assert_eq!(
        plus_calls(&mut hydrated, GRAFTS_PLUS),
        0,
        "an image boot leaves the call to + a call"
    );
}

/// Every code object `src` compiles to on `rt`, as its opcode names and its
/// length in bytes: the entry, then each child in table order, recursively.
fn instruction_streams(rt: &mut Runtime, src: &str) -> Vec<(Vec<String>, usize)> {
    fn walk(t: &crate::value::ClosureTemplate, out: &mut Vec<(Vec<String>, usize)>) {
        let bytes = t.bytecode();
        let opcodes = crate::compiler::bytecode::disassemble_lines(bytes)
            .iter()
            .map(|line| {
                let (_, rest) = line.split_once("] ").expect("a disassembled line");
                rest.split(' ').next().unwrap_or_default().to_string()
            })
            .collect();
        out.push((opcodes, bytes.len()));
        for i in 0..t.num_children() {
            walk(&t.child(i), out);
        }
    }
    let (_vm, symbols, cctx) = rt.parts();
    let unit =
        crate::pipeline::compile_file(src, symbols, cctx, "<parity>").expect("the file compiles");
    let mut out = Vec::new();
    walk(unit.entry(), &mut out);
    out
}

// docs/impl/image/pins.md: the same user file compiles to the same
// instruction stream under both boots.
//
// The comparison reads opcodes and lengths rather than bytes: a static region
// id in an operand comes from a process-wide mint that a stdlib compile
// advances and a hydration does not. A compile with the graft off is the
// control, so a file the graft leaves alone fails here rather than making the
// two boots agree vacuously.
#[test]
fn the_two_boots_emit_one_bytecode_for_a_user_file() {
    let scratch = Scratch::new("parity");
    let mut source = Runtime::with_caches(scratch.caches());
    assert_eq!(source.boot_source(), BootSource::Compiled);
    let compiled = instruction_streams(&mut source, GRAFTS_PLUS);
    drop(source);

    let mut ungrafted = Runtime::new();
    ungrafted
        .compile()
        .set_graft(crate::config::GraftPolicy::Off);
    assert_ne!(
        instruction_streams(&mut ungrafted, GRAFTS_PLUS),
        compiled,
        "the graft changes nothing in this file, so parity here proves nothing"
    );
    drop(ungrafted);

    let mut hydrated = Runtime::with_caches(scratch.caches());
    assert_eq!(hydrated.boot_source(), BootSource::Image);
    assert_eq!(
        instruction_streams(&mut hydrated, GRAFTS_PLUS),
        compiled,
        "an image boot compiles the file to other instructions"
    );
}
