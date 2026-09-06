// audited: 2026-09-06
// docs/impl/wasm.md
//! The module cache is a cache: an entry it cannot use is a miss.

/// A module small enough to compile instantly. `Module::new` reads WAT text as
/// readily as binary, so the cache path needs no binary fixture.
const FIXTURE_WAT: &str = "(module (func (export \"f\") (result i32) i32.const 7))";

// The trap: a `--cache` entry is named for a hash of the WASM bytes, and
// wasmtime's `Module::deserialize` accepts an artifact only from the version
// that wrote it. So the name can hit while the bytes are unusable, and the
// two conditions are indistinguishable to the caller until deserialize runs.
//
// The counter-factual is a run that treats that as an error rather than a
// miss. It reports `Module was compiled with incompatible version 'N'` and
// nothing ever deletes the entry, so every later run reads the same bytes and
// stops the same way — a wasmtime bump strands every warm cache until someone
// clears the directory by hand, with an error that never names one.
// Poisoned bytes stand in for the version mismatch here: deserialize refuses
// both, by the same path, without pinning the test to a wasmtime version.
#[test]
fn cache_entry_that_cannot_be_deserialized_recompiles() {
    let dir = tempfile::tempdir().expect("a temp cache directory");
    let path = dir.path().join("module_dead0000beef0000.bin");
    let engine = wasmtime::Engine::default();
    let wasm = FIXTURE_WAT.as_bytes();

    // A cold cache compiles and leaves an artifact behind.
    super::super::store::cached_or_compile(&engine, wasm, Some(&path)).expect("cold compile");
    assert!(
        path.exists(),
        "a cold compile must populate the cache entry"
    );
    let cached = std::fs::read(&path).expect("read the artifact");

    std::fs::write(&path, b"not a wasmtime artifact").expect("poison the entry");
    super::super::store::cached_or_compile(&engine, wasm, Some(&path))
        .expect("an unusable cache entry is a miss, not a failed compile");

    let repaired = std::fs::read(&path).expect("read the repaired artifact");
    assert_ne!(
        repaired, b"not a wasmtime artifact",
        "the miss must overwrite the unusable entry, or every later run repeats it"
    );
    assert_eq!(
        repaired, cached,
        "the repaired entry must be the artifact a cold compile writes"
    );
}

// No cache directory configured is the same compile without the disk round
// trip — the `--cache`-less default every plain `--wasm=full` run takes.
#[test]
fn uncached_compile_needs_no_cache_path() {
    let engine = wasmtime::Engine::default();
    let wasm = FIXTURE_WAT.as_bytes();
    super::super::store::cached_or_compile(&engine, wasm, None).expect("compile without a cache");
}
