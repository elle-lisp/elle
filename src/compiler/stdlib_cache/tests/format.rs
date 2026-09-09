// audited: 2026-09-09
// What the serialized form carries across, and the one thing a restored
// template loses.
// docs/impl/stdlib-cache.md

use super::super::*;
use crate::pipeline::compile_file;
use crate::primitives::module_init::StdlibSource;
use crate::runtime::Runtime;

/// Compile a snippet through the full pipeline, then assert that
/// store→load round-trips to an equivalent `Bytecode` (equal instructions
/// and constants, closures rebuilt, LIR preserved).
#[test]
fn bytecode_roundtrip_preserves_lir_and_closures() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Its own directory: this test writes a cache and must not read, write,
    // or be read by whatever else the suite is running beside it.
    let mut rt = Runtime::with_stdlib_cache(StdlibCache::Dir(dir.path().to_path_buf()));
    let (result, loaded) = {
        let (vm, symbols, cctx) = rt.parts();
        let src = r#"
(defn helper [x] (+ x 1))
(+ (helper 1) (helper 2))
"#;
        let result = compile_file(src, symbols, cctx, "<test>").expect("compiles");
        let bc = &result.bytecode;
        assert!(!bc.instructions.is_empty());

        let stored = store_bytecode(bc, vm, symbols, cctx).expect("stores");
        let stored_names = stored.names.len();
        let bytes = bincode::serialize(&stored).expect("serializes");
        let decoded: StoredBytecode = bincode::deserialize(&bytes).expect("deserializes");
        assert_eq!(
            decoded.names.len(),
            stored_names,
            "the spelling table survives bincode; without it every reloaded \
             symbol prints as #<symbol:hash>"
        );
        let loaded = load_bytecode(decoded, vm, symbols, cctx).expect("loads");
        assert_eq!(loaded.instructions, bc.instructions, "instructions equal");
        assert_eq!(loaded.signal, bc.signal);
        assert_eq!(loaded.child_protos.len(), bc.child_protos.len());
        // The constant pool is byte-identical on the scalar prefix; closure
        // constants are NEW heap instances after reload (pointer-equal
        // comparison would spuriously fail), so compare scalar kinds/counts.
        assert_eq!(
            bc.constants.len(),
            loaded.constants.len(),
            "same number of constants"
        );
        for (a, b) in bc.constants.iter().zip(&loaded.constants) {
            assert_eq!(a.is_closure(), b.is_closure(), "closure-ness preserved");
            assert_eq!(a.is_heap(), b.is_heap(), "heap-ness preserved");
        }
        // LIR must survive (JIT depends on it) and closures must be rebuilt.
        for (orig, reloaded) in bc.child_protos.iter().zip(&loaded.child_protos) {
            assert_eq!(
                orig.lir_function.is_some(),
                reloaded.lir_function.is_some(),
                "LIR presence preserved"
            );
        }
        let _ = vm;
        (result.bytecode, loaded)
    };
    // Both bytecodes must execute to the same result.
    let run = |bc: &crate::compiler::Bytecode| -> i64 {
        let (vm, _symbols, cctx) = rt.parts();
        vm.execute_scheduled(bc, cctx)
            .expect("runs")
            .as_int()
            .expect("result is an int")
    };
    let mut run = run;
    let r_orig = run(&result);
    let r_loaded = run(&loaded);
    assert_eq!(r_orig, r_loaded, "original and reloaded bytecode agree");
    eprintln!("roundtrip ok: {r_orig} == {r_loaded}");
}
/// The registry a cache hit restores must be the registry the stdlib
/// compile recorded. It drives an HIR rewrite in every later compile, so a
/// snapshot that drops entries makes the cached path compile user code
/// differently from the compiled path — and caching is on by default, so
/// the first run of a program and every run after it disagree.
///
/// The counter-factual: a snapshot that omits the templates whose body is a
/// `let` — the shape whose clone needed the defining arena — loses seven of
/// the stdlib's thirty-six, and the assertion names them.
#[test]
fn the_stored_inline_registry_keeps_every_template_the_compile_recorded() {
    use std::collections::BTreeSet;

    fn names(
        reg: &crate::hir::typeinfer::FnInlineRegistry,
        symbols: &crate::symbol::SymbolTable,
    ) -> BTreeSet<String> {
        reg.by_name
            .keys()
            .map(|n| symbols.name(*n).unwrap_or("?").to_string())
            .collect()
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let mut rt = Runtime::with_stdlib_cache(StdlibCache::Dir(dir.path().to_path_buf()));
    let (_vm, symbols, cctx) = rt.parts();

    let (recorded, stored) = {
        let (_, fn_inline) = cctx.compile_registries_mut();
        (names(fn_inline, symbols), fn_inline.to_stored(symbols))
    };
    assert!(
        !recorded.is_empty(),
        "the stdlib compile must record cross-unit inline templates, or \
         this test proves nothing about what the snapshot keeps"
    );

    let mut restored = crate::hir::typeinfer::FnInlineRegistry::default();
    restored.restore(stored, symbols);

    let survived = names(&restored, symbols);
    let lost: Vec<_> = recorded.difference(&survived).collect();
    assert!(
        lost.is_empty(),
        "a cache hit must inline what a stdlib compile inlines; these \
         templates do not survive the snapshot: {lost:?}"
    );
}

/// Two boot paths, one rewrite. `fn/cfg-label` is a stdlib `defn` whose body
/// is a `let` — the shape a registry that could not carry bindings had to
/// drop — and fusing it into a `map` splices that body into the emitted
/// loop, leaving no call to it at all.
///
/// The trap: the two paths' instruction streams are not byte-identical even
/// for `(+ 1 2)`, because bytecode operands carry ids from a process-global
/// mint counter that the stdlib compile advances and a cache hit does not.
/// So the claim is asserted where it lives — in the rewritten HIR — with the
/// stream lengths as corroboration.
#[test]
fn a_cache_hit_inlines_the_stdlib_bodies_a_stdlib_compile_inlines() {
    use crate::hir::{BindingArena, Hir, HirKind};
    use crate::pipeline::{compile_file_repl, compile_file_to_fhir};

    const SRC: &str = r#"(map fn/cfg-label [{:name "a"} {:name "b"}])"#;
    const INLINED: &str = "fn/cfg-label";

    fn calls_named(
        h: &Hir,
        arena: &BindingArena,
        symbols: &crate::symbol::SymbolTable,
        want: &str,
    ) -> usize {
        let mut n = 0;
        if let HirKind::Call { func, .. } = &h.kind {
            if let HirKind::Var(b) = &func.kind {
                n += usize::from(symbols.name(arena.get(*b).name) == Some(want));
            }
        }
        h.for_each_child(|c| n += calls_named(c, arena, symbols, want));
        n
    }

    fn probe(rt: &mut Runtime) -> (usize, usize) {
        let (_vm, symbols, cctx) = rt.parts();
        let (hir, arena) =
            compile_file_to_fhir(SRC, symbols, cctx, "<parity>").expect("compiles to HIR");
        let calls = calls_named(&hir, &arena, symbols, INLINED);
        let len = compile_file_repl(SRC, symbols, cctx, "<parity>")
            .expect("compiles")
            .0
            .bytecode
            .instructions
            .len();
        (calls, len)
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let cache = StdlibCache::Dir(dir.path().to_path_buf());

    let mut compiled = Runtime::with_stdlib_cache(cache.clone());
    assert_eq!(compiled.stdlib_source(), StdlibSource::Compiled);
    let (compiled_calls, compiled_len) = probe(&mut compiled);
    drop(compiled);

    let mut hit = Runtime::with_stdlib_cache(cache);
    assert_eq!(hit.stdlib_source(), StdlibSource::Cache);
    let (cached_calls, cached_len) = probe(&mut hit);

    assert_eq!(
        compiled_calls, 0,
        "the stdlib compile records the fragment, so the `map` fuses and \
         the call to `{INLINED}` is gone"
    );
    assert_eq!(
        cached_calls, 0,
        "a cache hit must fuse the same call; a registry that dropped the \
         fragment leaves the un-fused call behind"
    );
    assert_eq!(
        compiled_len, cached_len,
        "and the two paths must emit the same amount of code for it"
    );
}

/// `ClosureTemplate.origin` does not cross the cache — a restore rebuilds
/// templates from cached bytecode, not from the LIR the emitter set it on
/// — and `(meta/origin f)`, its only reader, reports a closure's source
/// location from it. So a stdlib closure has an origin on the compiled
/// path and none on the cached one. Nothing in the tree depends on that;
/// it is pinned here rather than left to be rediscovered as a surprise.
///
/// The second half is what bounds it: a closure the hit runtime compiles
/// itself still knows where it came from, so the loss stays with the values
/// the cache restored and does not reach user code.
#[test]
fn a_cached_stdlib_closure_has_no_origin_but_user_code_keeps_its_own() {
    fn origin_is_nil(rt: &mut Runtime, src: &str) -> bool {
        use crate::pipeline::compile_file_repl;
        let (vm, symbols, cctx) = rt.parts();
        let result = compile_file_repl(src, symbols, cctx, "<origin>").expect("compiles");
        vm.execute_scheduled(&result.0.bytecode, cctx)
            .expect("runs")
            .is_nil()
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let cache = StdlibCache::Dir(dir.path().to_path_buf());

    let mut compiled = Runtime::with_stdlib_cache(cache.clone());
    assert_eq!(compiled.stdlib_source(), StdlibSource::Compiled);
    assert!(
        !origin_is_nil(&mut compiled, "(meta/origin map)"),
        "a compiled stdlib closure carries its origin span"
    );
    drop(compiled);

    let mut hit = Runtime::with_stdlib_cache(cache);
    assert_eq!(hit.stdlib_source(), StdlibSource::Cache);
    assert!(
        origin_is_nil(&mut hit, "(meta/origin map)"),
        "the cache does not carry the origin span, so a restored closure \
         has none — a known difference between the two paths"
    );
    assert!(
        !origin_is_nil(&mut hit, "(meta/origin (fn [] nil))"),
        "the loss must stay with the restored values: a closure this \
         runtime compiled itself still knows where it came from"
    );
}
