// audited: 2026-11-25
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
    super::super::cache::cached_or_compile(&engine, wasm, Some(&path)).expect("cold compile");
    assert!(
        path.exists(),
        "a cold compile must populate the cache entry"
    );
    let cached = std::fs::read(&path).expect("read the artifact");

    std::fs::write(&path, b"not a wasmtime artifact").expect("poison the entry");
    super::super::cache::cached_or_compile(&engine, wasm, Some(&path))
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
    super::super::cache::cached_or_compile(&engine, wasm, None).expect("compile without a cache");
}

// A build's entries sit in a directory of its own beneath `wasm/`, and the
// first open removes every other build's (docs/impl/wasm.md). The
// counter-factual is the flat layout: every build wrote into the cache root,
// nothing removed an entry, and a week of rebuilds left 1.8 GB there.
#[test]
fn opening_a_builds_directory_removes_every_other_builds() {
    let root = tempfile::tempdir().expect("a temp cache root");
    let other = root.path().join("wasm").join("other");
    std::fs::create_dir_all(&other).expect("another build's directory");
    std::fs::write(other.join("module_0.bin"), b"x").expect("another build's entry");
    let boot = root.path().join("boot");
    std::fs::create_dir_all(&boot).expect("the warm cache");
    std::fs::write(boot.join("a.image"), b"x").expect("a warm-cache image");

    let dir =
        super::super::cache::open_build_dir(root.path(), "mine").expect("the build's directory");
    assert_eq!(dir, root.path().join("wasm").join("mine"));
    assert!(dir.is_dir(), "opening creates the build's directory");
    assert!(!other.exists(), "opening removes another build's directory");
    assert!(
        boot.join("a.image").exists(),
        "opening leaves the caches outside `wasm/` alone"
    );
}

// The counter-factual is an open that clears its own directory too, which
// would leave every run with a cold cache.
#[test]
fn reopening_a_builds_directory_keeps_its_entries() {
    let root = tempfile::tempdir().expect("a temp cache root");
    let dir =
        super::super::cache::open_build_dir(root.path(), "mine").expect("the build's directory");
    std::fs::write(dir.join("module_0.bin"), b"x").expect("an entry");
    super::super::cache::open_build_dir(root.path(), "mine").expect("the build's directory again");
    assert!(
        dir.join("module_0.bin").exists(),
        "a build keeps its own entries"
    );
}

// The counter-factual is the engine of the flat layout, handed the cache root:
// its compile cache wrote one file per function beside every other cache.
#[test]
fn an_engine_caches_its_compiles_in_the_directory_it_is_given() {
    let root = tempfile::tempdir().expect("a temp cache root");
    let dir = root.path().join("incremental");
    let engine = super::super::store::create_engine_in(Some(&dir)).expect("a wasmtime engine");
    wasmtime::Module::new(&engine, FIXTURE_WAT).expect("compile the fixture");
    let entries = std::fs::read_dir(&dir)
        .expect("the engine's cache directory")
        .count();
    assert!(
        entries > 0,
        "the compile left no entry in the engine's directory"
    );
    let beside: Vec<_> = std::fs::read_dir(root.path())
        .expect("the cache root")
        .map(|e| e.expect("an entry").file_name())
        .collect();
    assert_eq!(
        beside,
        ["incremental"],
        "the compile wrote outside the engine's directory"
    );
}

// The counter-factual is a name that ignores the build: every build shares the
// one directory, and no open ever removes another build's entries.
#[test]
fn a_builds_directory_is_named_for_its_identity() {
    use crate::image::boot::Executable;
    let name = super::super::cache::build_name;
    assert_eq!(name(&Executable::new(1, 2)), name(&Executable::new(1, 2)));
    assert_ne!(name(&Executable::new(1, 2)), name(&Executable::new(1, 3)));
    assert_ne!(name(&Executable::new(1, 2)), name(&Executable::new(2, 2)));
}
