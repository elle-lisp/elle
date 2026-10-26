// audited: 2026-10-26
//! What a boot image carries that only this crate can see, compile parity under either boot, and the warm cache's file name.
//!
//! docs/impl/image/boot.md

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::compiler::stdlib_cache::StdlibCache;
use crate::runtime::{BootCaches, Runtime};
use crate::scratch::ScratchDir;

use super::cache::Executable;
use super::{BootImage, BootSource};

/// A scratch directory for one test's caches.
fn boot_scratch(tag: &str) -> ScratchDir {
    ScratchDir::new(&format!("boot-{tag}"))
}

/// The stdlib cache and the boot image cache, both inside `scratch`.
fn caches(scratch: &ScratchDir) -> BootCaches {
    BootCaches {
        stdlib: StdlibCache::Dir(scratch.join("stdlib")),
        image: BootImage::Dir(scratch.join("boot")),
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

// docs/impl/image/sealing.md: the filled caches hydrate without recompiling,
// and a cache the boot never filled stays empty and fills lazily as today. So
// the table an image boot installs carries the same names with the same split
// as the source boot it was dumped from.
//
// The counter-factual is a dump that drops the caches: every name still
// arrives, every macro still expands, and each one silently recompiles its
// transformer in the first expansion context that asks — which is the
// mis-scoping the lazy fill exists to avoid.
#[test]
fn the_macro_table_crosses_with_its_filled_transformers() {
    let scratch = boot_scratch("macros");
    let mut source = Runtime::with_caches(caches(&scratch));
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

    let mut hydrated = Runtime::with_caches(caches(&scratch));
    assert_eq!(hydrated.boot_source(), BootSource::Image);
    assert_eq!(
        macro_shape(&mut hydrated),
        before,
        "the hydrated macro table differs from the one that was dumped"
    );
}

// docs/impl/image/format.md: the installed expander's counter clears every
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
    let scratch = boot_scratch("watermark");
    let counter = {
        let mut source = Runtime::with_caches(caches(&scratch));
        assert_eq!(source.boot_source(), BootSource::Compiled);
        source.compile().scope_counter()
    };

    let mut rt = Runtime::with_caches(caches(&scratch));
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
fn watermark_of(scratch: &ScratchDir) -> u32 {
    let path = std::fs::read_dir(scratch.join("boot"))
        .expect("the boot directory exists")
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "image"))
        .expect("a stored image");
    let mut probe = Runtime::without_stdlib();
    let (heap, symbols) = probe.heap_and_symbols();
    super::hydrate_path(heap, symbols, &path)
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
    // A REPL binding, so the call below reaches inc's closure through the
    // ordinary dispatch path.
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
    let scratch = boot_scratch("tier");
    let mut source = Runtime::with_caches(caches(&scratch));
    assert_eq!(source.boot_source(), BootSource::Compiled);
    assert!(
        inc_promotes(&mut source),
        "a source boot does not compile inc, so this test exercises nothing"
    );
    drop(source);

    let mut hydrated = Runtime::with_caches(caches(&scratch));
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
    let scratch = boot_scratch("graft");
    let mut source = Runtime::with_caches(caches(&scratch));
    assert_eq!(source.boot_source(), BootSource::Compiled);
    assert_eq!(
        plus_calls(&mut source, GRAFTS_PLUS),
        0,
        "a source boot does not graft +, so this test exercises nothing"
    );
    drop(source);

    let mut hydrated = Runtime::with_caches(caches(&scratch));
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
    let scratch = boot_scratch("parity");
    let mut source = Runtime::with_caches(caches(&scratch));
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

    let mut hydrated = Runtime::with_caches(caches(&scratch));
    assert_eq!(hydrated.boot_source(), BootSource::Image);
    assert_eq!(
        instruction_streams(&mut hydrated, GRAFTS_PLUS),
        compiled,
        "an image boot compiles the file to other instructions"
    );
}

// ── The warm cache's file name ───────────────────────────────────────

// docs/impl/image/boot.md: one executable rebuilt gives one source digest
// two names. The counter-factual is a name from the digest alone: the new
// build opens the image the old one stored, and runs the code the old build
// compiled.
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

// docs/impl/image/boot.md: a binary that cannot read its own identity reads
// and writes no image, as the stdlib cache declines to cache. Falling back to
// the digest alone would bring back the confusion the identity exists to
// prevent.
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
