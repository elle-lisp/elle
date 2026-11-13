// audited: 2026-11-13
//! The cache file on disk: where it lives, how a corrupt one loads, and how a
//! store replaces it.
//!
//! docs/impl/stdlib-cache.md

use super::super::*;
use crate::primitives::module_init::StdlibSource;
use crate::runtime::Runtime;

/// Two runtimes over one cache directory: the first compiles stdlib and
/// writes the cache, and the second loads it and still runs stdlib code.
#[test]
fn second_runtime_on_a_shared_cache_dir_loads_stdlib_from_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = StdlibCache::Dir(dir.path().to_path_buf());

    let mut a = Runtime::with_stdlib_cache(cache.clone());
    assert_eq!(
        a.stdlib_source(),
        StdlibSource::Compiled,
        "the first runtime meets an empty directory, so it must compile"
    );

    let mut b = Runtime::with_stdlib_cache(cache);
    assert_eq!(
        b.stdlib_source(),
        StdlibSource::Cache,
        "the second runtime must load what the first wrote"
    );

    // Both runtimes run a stdlib function.
    let probe = |rt: &mut Runtime| -> crate::value::Value {
        use crate::pipeline::compile_file_repl;
        let (vm, symbols, cctx) = rt.parts();
        let src = "(map (fn [x] (* x 2)) (quote (1 2 3)))";
        let result = compile_file_repl(src, symbols, cctx, "<probe>").expect("probe compiles");
        vm.execute_scheduled(&result.0, cctx).expect("probe runs")
    };
    let _ = probe(&mut a);
    let _ = probe(&mut b);
}

/// A `sys/spawn` worker runs `init_stdlib` on its own thread, so it reads
/// and writes a cache of its own. It must use the directory its parent was
/// given. A worker that falls back to the process-wide directory writes
/// megabytes into a place nobody named.
#[test]
fn a_spawned_worker_caches_where_its_parent_was_told_to() {
    use crate::pipeline::compile_file_repl;

    let dir = tempfile::tempdir().expect("tempdir");
    let mut rt = Runtime::with_stdlib_cache(StdlibCache::Dir(dir.path().to_path_buf()));

    // The parent's own store already filled the directory, and the worker
    // writes the same key. Clear it, so anything present afterwards can
    // only have been written by the worker.
    let entries =
        |p: &std::path::Path| -> usize { std::fs::read_dir(p).expect("read cache dir").count() };
    for entry in std::fs::read_dir(dir.path()).expect("read cache dir") {
        std::fs::remove_file(entry.expect("entry").path()).expect("clear");
    }
    assert_eq!(entries(dir.path()), 0, "cleared");

    let (vm, symbols, cctx) = rt.parts();
    let result = compile_file_repl("(sys/join (sys/spawn (fn [] 1)))", symbols, cctx, "<spawn>")
        .expect("spawn form compiles");
    vm.execute_scheduled(&result.0, cctx).expect("spawn runs");

    assert_eq!(
        entries(dir.path()),
        1,
        "the worker must cache into the directory its parent was given, \
         not the process-wide one"
    );
}

/// A cache file is bytes on a disk any process can write. Every corruption
/// must load as a miss, never as a hit or a panic, because a full compile is
/// always available.
///
/// The test flips each byte rather than writing a fixed one. The payload
/// holds runs of `0xFF`, and a fill of `0xFF` over such a run leaves the file
/// unchanged, so a hit is then the correct result.
#[test]
fn a_corrupt_cache_file_is_a_miss_not_a_panic_or_a_silent_edit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = StdlibCache::Dir(dir.path().to_path_buf());

    // Seed a good cache, and keep its bytes as the base of each round.
    drop(Runtime::with_stdlib_cache(cache.clone()));
    let path = std::fs::read_dir(dir.path())
        .expect("read cache dir")
        .next()
        .expect("the seeding runtime wrote a cache file")
        .expect("entry")
        .path();
    let good = std::fs::read(&path).expect("read cache file");
    assert!(
        good.len() > 8192,
        "cache file too small to corrupt meaningfully"
    );

    // Two offsets, because there are two failure modes: a shallow one
    // lands in the decoded structure and reaches the VM as instructions, a
    // deep one lands in a payload and only the hash notices it. More
    // offsets in the shallow band would repeat a mode rather than add one,
    // and each round costs a runtime boot.
    for offset in [64usize, good.len() / 2] {
        let mut bad = good.clone();
        for byte in &mut bad[offset..offset + 8] {
            *byte = !*byte;
        }
        std::fs::write(&path, &bad).expect("write corrupt cache");

        let rt = Runtime::with_stdlib_cache(cache.clone());
        assert_eq!(
            rt.stdlib_source(),
            StdlibSource::Compiled,
            "eight corrupt bytes at offset {offset} must be a miss"
        );
    }
}

/// A start that rejects the file must also replace it. A rejected file left
/// on disk is rejected again by every later start, so one bad write costs the
/// cache for good. A miss still yields a working runtime, so only
/// `stdlib_source` shows the recompile.
#[test]
fn a_rejected_cache_file_is_replaced_not_left_to_win() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = StdlibCache::Dir(dir.path().to_path_buf());

    drop(Runtime::with_stdlib_cache(cache.clone()));
    let path = std::fs::read_dir(dir.path())
        .expect("read cache dir")
        .next()
        .expect("the seeding runtime wrote a cache file")
        .expect("entry")
        .path();
    std::fs::write(&path, b"not a cache file").expect("plant a rejected file");

    let first = Runtime::with_stdlib_cache(cache.clone());
    assert_eq!(
        first.stdlib_source(),
        StdlibSource::Compiled,
        "the planted file must be rejected"
    );
    drop(first);

    let second = Runtime::with_stdlib_cache(cache);
    assert_eq!(
        second.stdlib_source(),
        StdlibSource::Cache,
        "the runtime that rejected the file must also replace it, or every \
         later start pays the full compile again"
    );
}

/// A store lands whole or not at all.
///
/// A held descriptor is the observable: writing the path in place edits the
/// inode the reader already has, while a rename leaves that inode alone and
/// swings the name to a new one.
#[test]
fn a_store_replaces_the_cache_file_instead_of_rewriting_it() {
    use std::io::Read;

    let dir = tempfile::tempdir().expect("tempdir");
    let cache = StdlibCache::Dir(dir.path().to_path_buf());

    drop(Runtime::with_stdlib_cache(cache.clone()));
    let path = std::fs::read_dir(dir.path())
        .expect("read cache dir")
        .next()
        .expect("the seeding runtime wrote a cache file")
        .expect("entry")
        .path();

    // Unreadable, so the next runtime rejects it and must store over it.
    const SENTINEL: &[u8] = b"the inode a reader already holds";
    std::fs::write(&path, SENTINEL).expect("plant a rejected file");
    let mut held = std::fs::File::open(&path).expect("hold the old inode open");

    drop(Runtime::with_stdlib_cache(cache));

    let mut seen = Vec::new();
    held.read_to_end(&mut seen)
        .expect("read through the held fd");
    // Compared as a bool: the rewritten file is megabytes, and dumping it
    // into the failure would bury the one fact that matters.
    assert!(
        seen == SENTINEL,
        "the store must rename a complete file into place; rewriting the \
         path edits the inode another process is already reading — the \
         held descriptor saw {} bytes, not the {}-byte sentinel",
        seen.len(),
        SENTINEL.len()
    );
}

/// A store removes every file under a key this binary no longer mints.
/// Without the prune, each rebuild leaves its megabytes behind.
#[test]
fn a_store_prunes_the_files_its_key_supersedes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = StdlibCache::Dir(dir.path().to_path_buf());

    // Two files under keys this binary will never mint again.
    for name in ["deadbeefdeadbeef.bin", "0123456789abcdef.bin"] {
        std::fs::write(dir.path().join(name), b"an earlier build's cache")
            .expect("plant a superseded file");
    }

    drop(Runtime::with_stdlib_cache(cache));

    let left: Vec<_> = std::fs::read_dir(dir.path())
        .expect("read cache dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(
        left.len(),
        1,
        "a store must leave only the file it just wrote; found {left:?}"
    );
}
