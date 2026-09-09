// audited: 2026-09-09
// The file on disk: which key names it, which directory holds it, and what a
// store owes a process that is reading the file it replaces.
// docs/impl/stdlib-cache.md

use super::super::*;
use crate::primitives::module_init::StdlibSource;
use crate::runtime::Runtime;

/// The key must follow the binary, not its version string: two builds of
/// one version compile stdlib differently the moment a pass changes. This
/// pins that the identity is read from the executable — an edit that put a
/// constant back would leave every rebuild sharing one key, and every test
/// that boots a runtime reading bytecode the previous binary produced.
///
/// Characterization, not a failing-first regression: "a different build
/// yields a different key" is observable only across builds.
#[test]
fn the_build_identity_is_read_from_the_running_executable() {
    let (len, _mtime) = build_identity().expect("the test binary can identify itself");
    let exe = std::env::current_exe().expect("current_exe");
    let meta = std::fs::metadata(&exe).expect("exe metadata");
    assert_eq!(
        len,
        meta.len(),
        "the identity must be the binary's own size"
    );
    assert!(len > 0, "a zero-length identity separates nothing");
}

/// Two runtimes over one cache directory: the first compiles stdlib and
/// writes the cache, the second must load from it — and must still have a
/// working stdlib afterwards.
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
        "the second runtime must load what the first wrote; a cache that \
         silently never hits still yields a working runtime, so behaviour \
         alone cannot tell the two apart"
    );

    // Working stdlib on both sides. Functional check only — timing is
    // asserted in the release-mode boot benchmark instead (debug builds
    // skew it).
    let probe = |rt: &mut Runtime| -> crate::value::Value {
        use crate::pipeline::compile_file_repl;
        let (vm, symbols, cctx) = rt.parts();
        let src = "(map (fn [x] (* x 2)) (quote (1 2 3)))";
        let result = compile_file_repl(src, symbols, cctx, "<probe>").expect("probe compiles");
        vm.execute_scheduled(&result.0.bytecode, cctx)
            .expect("probe runs")
    };
    let _ = probe(&mut a);
    let _ = probe(&mut b);
}

/// A `sys/spawn` worker runs `init_stdlib` on its own thread, so it reads
/// and writes a cache of its own. It must use the directory its parent was
/// given: a worker that falls back to the process-wide one writes megabytes
/// into a place nobody named, which is the leak the construction parameter
/// exists to close.
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
    vm.execute_scheduled(&result.0.bytecode, cctx)
        .expect("spawn runs");

    assert_eq!(
        entries(dir.path()),
        1,
        "the worker must cache into the directory its parent was given, \
         not the process-wide one"
    );
}

/// A cache file is bytes on a disk any process can write. `bincode` reports
/// only that the bytes *decoded*, never that they are the bytes this binary
/// wrote — so a flipped byte reaches the VM as instructions, and a deeper
/// one is absorbed into stdlib and reported as a hit. Every corruption must
/// come out as a miss: the cache is an optimization, and a full compile is
/// always available.
#[test]
fn a_corrupt_cache_file_is_a_miss_not_a_panic_or_a_silent_edit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = StdlibCache::Dir(dir.path().to_path_buf());

    // Seed a good cache, then keep its bytes to restore between rounds.
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
    // deep one lands in a payload and passes unnoticed. More offsets in the
    // shallow band would repeat a mode rather than add one, and each round
    // costs a runtime boot.
    for offset in [64usize, good.len() / 2] {
        let mut bad = good.clone();
        for byte in &mut bad[offset..offset + 8] {
            *byte = 0xFF;
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

/// Falling back is half the job. A rejected file that stays on disk is
/// rejected again by every later start, so one bad write costs the cache
/// permanently — the recompile it forces is invisible, because a working
/// runtime is what a miss produces too.
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

/// Two elle processes starting at once is an ordinary event, and a store
/// that writes the final path directly lets one of them read the other's
/// half-written file. The store must land whole or not at all.
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

/// Every key that stops being current orphans a file, and the key follows
/// the binary — so an ordinary day of rebuilds leaves one 16 MB file per
/// build, forever, in a directory nobody thinks to look at.
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
