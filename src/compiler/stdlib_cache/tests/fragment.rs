// audited: 2026-10-21
//! A function the cache restores carries its body's fragment, so a later compile
//! grafts it as it grafts a compiled one.
//!
//! docs/impl/stdlib-cache.md
//! docs/impl/graft.md

use super::super::*;
use crate::hir::testkit::count_calls_to;
use crate::primitives::module_init::StdlibSource;
use crate::runtime::Runtime;

/// The calls to `+` left in `src` once `rt` has compiled it.
fn plus_calls(rt: &mut Runtime, src: &str) -> usize {
    let (_vm, symbols, cctx) = rt.parts();
    let (hir, arena) = crate::pipeline::compile_file_to_fhir(src, symbols, cctx, "<probe>")
        .expect("the probe compiles");
    count_calls_to(&hir, &arena, "+")
}

/// Two runtimes over one directory: the first compiles the stdlib, and the
/// second restores it from the file the first wrote. Both must graft `+`.
///
/// The counter-factual: a restore that drops the fragment leaves the second
/// runtime's `+` a closure with no body to read. Its call stays a call, and a
/// cache hit compiles user code differently from a cache miss.
#[test]
fn a_restored_function_grafts_as_a_compiled_one_does() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = StdlibCache::Dir(dir.path().to_path_buf());
    let src = "(defn h [a b] (+ a b)) (h 1 2)";

    let mut compiled = Runtime::with_stdlib_cache(cache.clone());
    assert_eq!(compiled.stdlib_source(), StdlibSource::Compiled);
    assert_eq!(
        plus_calls(&mut compiled, src),
        0,
        "a compiled stdlib grafts +"
    );

    let mut restored = Runtime::with_stdlib_cache(cache);
    assert_eq!(restored.stdlib_source(), StdlibSource::Cache);
    assert_eq!(
        plus_calls(&mut restored, src),
        0,
        "a restored stdlib grafts +"
    );
}
