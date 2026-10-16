// audited: 2026-10-14
//! What the serialized stdlib carries across, and the one thing a restored code object loses.
//!
//! docs/impl/stdlib-cache.md

use super::super::*;
use crate::pipeline::compile_file;
use crate::primitives::module_init::StdlibSource;
use crate::runtime::Runtime;

/// Compile a snippet through the full pipeline, then assert that
/// store→load round-trips to an equivalent code unit (equal entry bytecode and
/// constants, closures rebuilt, every nested lambda's LIR preserved).
#[test]
fn bytecode_roundtrip_preserves_lir_and_closures() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Its own directory: this test writes a cache and must not read, write,
    // or be read by whatever else the suite is running beside it.
    let mut rt = Runtime::with_stdlib_cache(StdlibCache::Dir(dir.path().to_path_buf()));
    let (unit, loaded) = {
        let (vm, symbols, cctx) = rt.parts();
        let src = r#"
(defn helper [x] (+ x 1))
(+ (helper 1) (helper 2))
"#;
        let unit = compile_file(src, symbols, cctx, "<test>").expect("compiles");
        assert!(!unit.entry().bytecode().is_empty());

        let stored = store_bytecode(&unit, vm, symbols).expect("stores");
        let stored_names = stored.names.len();
        let bytes = bincode::serialize(&stored).expect("serializes");
        let decoded: StoredBytecode = bincode::deserialize(&bytes).expect("deserializes");
        assert_eq!(
            decoded.names.len(),
            stored_names,
            "the spelling table survives bincode; without it every reloaded \
             symbol prints as #<symbol:hash>"
        );
        let loaded = load_bytecode(decoded, vm, symbols).expect("loads");
        let (a, b) = (unit.entry(), loaded.entry());
        assert_eq!(b.bytecode(), a.bytecode(), "instructions equal");
        assert_eq!(b.signal(), a.signal());
        assert_eq!(b.num_children(), a.num_children());
        // The constant pool is byte-identical on the scalar prefix; closure
        // constants are NEW heap instances after reload (pointer-equal
        // comparison would spuriously fail), so compare scalar kinds/counts.
        assert_eq!(
            a.constants().len(),
            b.constants().len(),
            "same number of constants"
        );
        for (x, y) in a.constants().iter().zip(b.constants()) {
            assert_eq!(x.is_closure(), y.is_closure(), "closure-ness preserved");
            assert_eq!(x.is_heap(), y.is_heap(), "heap-ness preserved");
        }
        // LIR must survive (the JIT depends on it): every nested lambda the
        // reloaded unit carries runs the instructions the compiled one does.
        let ops = |v: &crate::lir::LirView<'_>| v.nodes().map(|n| n.op()).collect::<Vec<_>>();
        for i in 0..a.num_children() {
            let (orig, reloaded) = (a.child(i), b.child(i));
            let orig = orig.lir().expect("a nested lambda has LIR");
            let reloaded = reloaded.lir().expect("the reloaded lambda carries LIR");
            assert_eq!(
                ops(&orig),
                ops(&reloaded),
                "the reloaded LIR runs different instructions"
            );
        }
        (unit, loaded)
    };
    // Both units must execute to the same result.
    let mut run = |unit: &crate::value::CodeUnit| -> i64 {
        let (vm, _symbols, cctx) = rt.parts();
        vm.execute_scheduled(unit, cctx)
            .expect("runs")
            .as_int()
            .expect("result is an int")
    };
    let r_orig = run(&unit);
    let r_loaded = run(&loaded);
    assert_eq!(r_orig, r_loaded, "original and reloaded units agree");
}

/// Two boot paths, one rewrite. `fn/cfg-label` is a stdlib `defn` whose body
/// is a `let`, which binds names the fragment must carry. Fusing it into a
/// `map` splices that body into the emitted loop, leaving no call to it at all.
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
            .entry()
            .bytecode()
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
        "a cache hit must fuse the same call; a cache that dropped the \
         fragment leaves the un-fused call behind"
    );
    assert_eq!(
        compiled_len, cached_len,
        "and the two paths must emit the same amount of code for it"
    );
}

/// A cache hit's stdlib closures reach the JIT as a compiled stdlib's do: the
/// restored templates carry their LIR into the payload every reader reads.
/// The counter-factual is a hit whose closures run interpreted forever, which
/// answers every call correctly and is visible only here and in a profile.
#[test]
fn a_cached_stdlib_closure_carries_its_lir() {
    fn map_has_lir(rt: &mut Runtime) -> bool {
        let (vm, symbols, cctx) = rt.parts();
        let map = crate::pipeline::eval_all("map", symbols, vm, cctx, "<lir>").expect("map");
        map.as_closure()
            .expect("map is a closure")
            .template
            .lir()
            .is_some()
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let cache = StdlibCache::Dir(dir.path().to_path_buf());

    let mut compiled = Runtime::with_stdlib_cache(cache.clone());
    assert_eq!(compiled.stdlib_source(), StdlibSource::Compiled);
    assert!(
        map_has_lir(&mut compiled),
        "a compiled stdlib's map has LIR"
    );
    drop(compiled);

    let mut hit = Runtime::with_stdlib_cache(cache);
    assert_eq!(hit.stdlib_source(), StdlibSource::Cache);
    assert!(
        map_has_lir(&mut hit),
        "a cache hit's map lost its LIR, so it never reaches the JIT"
    );
}

/// The stored form of a code object carries its body fragment's bytes as the
/// payload holds them, so a load writes them back as they stand and a later
/// compile grafts the restored function (docs/impl/stdlib-cache.md). The bytes
/// are the `LambdaFragment` itself, so they decode back to one.
///
/// The counter-factual is a stored form that drops the field: every other pin
/// here still passes, and only the end-to-end graft in `fragment.rs` sees it.
#[test]
fn a_stored_code_object_carries_its_fragment_bytes() {
    let mut rt = Runtime::new();
    let (vm, symbols, cctx) = rt.parts();
    let unit = compile_file(
        "(defn inc2 [x] (+ x 2)) (inc2 1)",
        symbols,
        cctx,
        "<format>",
    )
    .expect("compiles");
    let compiled = (0..unit.entry().num_children())
        .map(|i| unit.entry().child(i))
        .find(|c| c.name() == Some("inc2"))
        .expect("the unit builds inc2");
    assert!(
        !compiled.fragment().is_empty(),
        "inc2's body closes, so its payload carries a fragment"
    );

    let stored = store_bytecode(&unit, vm, symbols).expect("stores");
    let inc2 = stored
        .entry
        .child_protos
        .iter()
        .find(|c| c.name.as_deref() == Some("inc2"))
        .expect("inc2 is stored");
    assert_eq!(
        inc2.fragment,
        compiled.fragment(),
        "the stored form carries the payload's bytes as they stand"
    );
    let decoded: crate::hir::fragment::LambdaFragment =
        bincode::deserialize(&inc2.fragment).expect("the bytes are a LambdaFragment");
    assert_eq!(decoded.fixed(), 1, "inc2 takes one parameter");
}

/// A payload's origin does not cross the cache — a restore rebuilds code
/// objects from the stored form, which carries no field for it — and
/// `(meta/origin f)`, its only reader, reports a closure's source
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
        vm.execute_scheduled(&result.0, cctx)
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
