//! Disk cache for the standard-library compilation.
//!
//! `init_stdlib` recompiles stdlib.lisp (~2850 lines) on every process start,
//! costing ~2.4s in `compile_file` before execution (5ms) even runs. The whole
//! front end (expand → analyze → regions → lower → emit) is deterministic:
//! same stdlib source, same elle binary → same bytecode. This module turns that
//! work into a one-time cost by serializing the compiled `Bytecode` (plus the
//! per-closure `ClosureTemplate`s and their LIR, so the JIT keeps working) to a
//! content-addressed cache file, keyed by the running binary's identity +
//! stdlib source hash + the canonical primitive-table identity (the last
//! because serialized native-fn immediates carry process-local `prim_id`s).
//!
//! Serialization strategy: the cache format is a plain `StoredBytecode` struct
//! that is 100% owned data — no `Rc`, no pointers. A symbol or keyword id is
//! its name's hash (docs/impl/symbol.md), the same number in every process, so
//! the ids travel as they stand; only the spellings ride alongside, in the
//! `names` table, and replay into the loading instance's display memo.
//! `Value`s that appear in the constant pool are scalars
//! (int/float/bool/nil/keyword/symbol) by construction — string and compound
//! literals lower to `MaterializeConst` templates, not pool constants — so the
//! pool serializes cheaply. Closures recurse via `child_protos`.
//!
//! LIR: the JIT compiles from `ClosureTemplate.lir_function` in the background.
//! If the cache dropped LIR, every stdlib function would run interpreted
//! forever (no LIR → never submitted to the JIT worker) — a silent runtime
//! regression, so LIR is serialized too, with its `doc`/`syntax` Rc fields
//! skipped (they are already `None` after the cross-thread conversion in
//! `sendable_from_template`; JIT never reads them).

use crate::compiler::Bytecode;
use crate::signals::Signal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::rc::Rc;

/// Version tag: bump when the serialized layout changes in an incompatible way.
const FORMAT_VERSION: u32 = 5;

/// Bytes of payload hash a cache file carries ahead of its `StoredBytecode`.
const PAYLOAD_HASH_BYTES: usize = 8;

/// Hash of a cache file's payload, stored in its prefix and re-checked on load.
///
/// `bincode` reports that bytes *decoded*, never that they are the bytes this
/// binary wrote: eight flipped bytes decode "successfully" and arrive at the VM
/// as instructions, and a flip deeper in a payload is absorbed into stdlib and
/// reported as a hit. The prefix is what distinguishes the two.
///
/// This detects corruption and truncation, not forgery. A writable cache
/// directory is a code-execution surface like any other loadable artifact; the
/// hash is not a defence against someone who can write there.
fn payload_hash(bytes: &[u8]) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write(bytes);
    hasher.finish()
}

/// The on-disk form of a compiled module's entry `Bytecode`.
///
/// The whole `Bytecode` is wrapped as a synthetic entry `ClosureTemplate` and
/// serialized through the send module's template path (`serialize_templates`),
/// which deep-copies the entry constant pool (it may contain live closure
/// instances — stdlib's `init_stdlib` result closure, `map`, etc.), the
/// nested-lambda blueprints, their LIR, and the region-release tables. The
/// two extra fields below (`signal_projection`, `format_version`) don't exist
/// on `ClosureTemplate`, so they ride alongside.
#[derive(Serialize, Deserialize)]
pub struct StoredBytecode {
    pub format_version: u32,
    /// The entry template: `instructions` → bytecode, `constants` → entry
    /// pool, `child_protos` → nested lambdas. LIR preserved.
    pub entry: crate::value::send::SendableClosure,
    /// Intern table of closure constants reachable from the entry's pool and
    /// its child templates, referenced by `Ref(idx)`.
    pub intern_table: Vec<crate::value::send::SendableClosure>,
    /// Spelling table for every symbol and keyword the entry and its templates
    /// name, replayed into the loading instance's display memo. The ids are
    /// name hashes and cross unchanged; without this table they would still
    /// compare correctly but print as `#<symbol:hash>`.
    pub names: Vec<(u64, Box<str>)>,
    pub signal_projection: Option<HashMap<String, Signal>>,
    /// Cross-unit dispatch-wrapper registry (stdlib `push`/`put`/`add`
    /// monomorphization), snapshotted because the disk cache skips the stdlib
    /// compile that would otherwise populate it.
    pub(crate) dispatch_wrappers: crate::hir::typeinfer::StoredDispatchRegistry,
    /// Cross-unit inline-fn registry (stdlib `inc`/`dec`/… HOF-argument
    /// inlining), likewise snapshotted. Its entries are `HirFragment`s — bodies
    /// closed over their own binding tables — so the whole registry crosses,
    /// and a cache hit compiles user code the way a stdlib compile does.
    pub(crate) fn_inline: crate::hir::typeinfer::StoredFnInlineRegistry,
}
/// Where a runtime caches its compiled stdlib.
///
/// This is a construction parameter, not process-global state. A `Runtime` is
/// built per instance and the suite builds many of them across threads, so the
/// directory has to travel with the instance that uses it: a global would make
/// one test's cache visible to every other test running beside it, and the
/// runtime that wrote it indistinguishable from the runtime that read it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StdlibCache {
    /// The process-wide choice: `stdlib-cache` beneath the `--cache=<dir>`
    /// directory. `--cache=` (empty) turns caching off for the process.
    #[default]
    Process,
    /// Cache in this directory, whatever the process-wide choice is.
    Dir(std::path::PathBuf),
    /// Never read or write a cache file; compile every time.
    Off,
}

impl StdlibCache {
    /// The directory to cache in, or `None` when caching is off.
    fn dir(&self) -> Option<std::path::PathBuf> {
        match self {
            StdlibCache::Off => None,
            StdlibCache::Dir(d) => Some(d.clone()),
            StdlibCache::Process => crate::config::get()
                .cache
                .as_ref()
                .map(|base| std::path::PathBuf::from(base).join("stdlib-cache")),
        }
    }
}

/// Identity of the running binary — its length and modification time.
///
/// Returns `None` when the executable cannot be located or measured. A binary
/// that cannot identify itself must not share a cache with one that can, and
/// falling back to the version string would restore the very confusion this
/// exists to prevent, so the caller declines to cache instead.
fn build_identity() -> Option<(u64, u128)> {
    let exe = std::env::current_exe().ok()?;
    let meta = std::fs::metadata(exe).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some((meta.len(), mtime))
}

/// Content hash of the stdlib source, the running binary's identity,
/// `FORMAT_VERSION`, and the primitive-table identity — the cache key.
///
/// `None` when the binary cannot be identified; the caller then neither reads
/// nor writes a cache file.
fn cache_key(stdlib_source: &str) -> Option<String> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    stdlib_source.hash(&mut hasher);
    // The binary itself, not its version string. Two builds of one version
    // compile stdlib differently the moment the emitter or a pass changes, and
    // a key that cannot see the rebuild hands every `Runtime::new()` — the one
    // in each test included — bytecode the previous binary produced. A test
    // failure would then stop implicating the branch that caused it.
    //
    // It also covers what the primitive-table identity below cannot see: the
    // ids `prim_id_of` appends outside the canonical tables (trait methods,
    // FFI callbacks) are minted by the running binary, not listed in it.
    build_identity()?.hash(&mut hasher);
    FORMAT_VERSION.hash(&mut hasher);
    // A serialized native-fn immediate carries a `prim_id`, which is only
    // valid against the exact primitive table that minted it. Mix the table
    // identity in so a prim addition/removal/reorder (a different elle
    // binary) invalidates the cache instead of deserializing a foreign id
    // into `panic!("unknown prim id")`.
    crate::primitives::registration::hash_prim_table_identity(&mut hasher);
    Some(format!("{:016x}.bin", hasher.finish()))
}

/// Try to load the compiled stdlib from the disk cache.
///
/// Returns `None` when no cache is enabled or the file is absent; `Some(Err)`
/// when the file exists but is rejected (hash mismatch, truncation, format
/// drift), naming the reason. `init_stdlib` treats a rejection exactly like an
/// absence — it compiles *and stores* — so the rejected file is replaced rather
/// than rejected again by every later start.
pub fn try_load(
    stdlib_source: &str,
    cache: &StdlibCache,
    vm: &mut crate::vm::VM,
    symbols: &mut crate::symbol::SymbolTable,
    cctx: &mut crate::pipeline::CompileCtx,
) -> Option<Result<Bytecode, String>> {
    let path = cache.dir()?.join(cache_key(stdlib_source)?);
    let bytes = std::fs::read(&path).ok()?;
    if bytes.len() < PAYLOAD_HASH_BYTES {
        return Some(Err("cache file shorter than its hash prefix".into()));
    }
    let (prefix, payload) = bytes.split_at(PAYLOAD_HASH_BYTES);
    let recorded = u64::from_le_bytes(prefix.try_into().expect("split at 8"));
    if recorded != payload_hash(payload) {
        return Some(Err("cache payload does not match its recorded hash".into()));
    }
    let stored: StoredBytecode = match bincode::deserialize(payload) {
        Ok(s) => s,
        Err(e) => return Some(Err(format!("cache decode: {e}"))),
    };
    Some(load_bytecode(stored, vm, symbols, cctx))
}

/// Store the compiled stdlib to the disk cache. Failures are ignored — the
/// cache is an optimization; a fresh compile is always valid.
pub fn try_store(
    stdlib_source: &str,
    cache: &StdlibCache,
    bytecode: &Bytecode,
    vm: &mut crate::vm::VM,
    symbols: &crate::symbol::SymbolTable,
    cctx: &mut crate::pipeline::CompileCtx,
) {
    let Some(dir) = cache.dir() else { return };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("[stdlib-cache] mkdir failed: {e}");
        return;
    }
    let stored = match store_bytecode(bytecode, vm, symbols, cctx) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[stdlib-cache] store failed: {e}");
            return;
        }
    };
    match bincode::serialize(&stored) {
        Ok(bytes) => {
            let Some(key) = cache_key(stdlib_source) else {
                return;
            };
            let path = dir.join(key);
            let mut file = payload_hash(&bytes).to_le_bytes().to_vec();
            file.extend_from_slice(&bytes);
            // Write beside the target and rename over it. Two elle processes
            // starting at once is ordinary, and writing the final path directly
            // lets one read the other's half-written file — or edits the inode a
            // reader already holds open. A rename is one atomic step within a
            // directory, so the name refers to a complete file or the old one.
            if let Err(e) = store_atomically(&dir, &path, &file) {
                eprintln!("[stdlib-cache] write failed: {e}");
                return;
            }
            prune_superseded(&dir, &path);
        }
        Err(e) => eprintln!("[stdlib-cache] serialize failed: {e}"),
    }
}

/// Write `bytes` to `path` by way of a temporary file in the same directory.
///
/// Same directory because a rename is atomic only within one filesystem; a
/// temp file elsewhere would fall back to a copy and reintroduce the torn read.
fn store_atomically(
    dir: &std::path::Path,
    path: &std::path::Path,
    bytes: &[u8],
) -> std::io::Result<()> {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.flush()?;
    // `persist` renames; on failure the temp file is removed with the error, so
    // a failed store leaves the directory as it found it.
    tmp.persist(path)
        .map(|_| ())
        .map_err(|e| std::io::Error::other(e.to_string()))
}

/// Remove every cache file in `dir` except `keep`.
///
/// The key follows the binary, so every rebuild mints a new one and orphans
/// the file the last one wrote. At ~16 MB each, a day of rebuilds fills a
/// directory nobody thinks to look at. Run after the rename, never before: a
/// store that fails must leave the directory as it found it, still holding a
/// file some other process may be about to read.
///
/// A removal that fails is ignored. It is disk hygiene, not correctness, and
/// the next store tries again.
fn prune_superseded(dir: &std::path::Path, keep: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path != keep && path.extension().is_some_and(|e| e == "bin") {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Serialize compiled stdlib bytecode into the cache format.
///
/// The whole `Bytecode` is wrapped as a synthetic entry `ClosureTemplate`
/// (arity `Exact(0)` — the entry runs as a thunk) and serialized through the
/// send module's template path. This handles everything uniformly: the entry
/// constant pool (which may hold live closure instances), the nested-lambda
/// blueprints, their LIR (so the JIT keeps working after reload), and the
/// region-release tables.
pub fn store_bytecode(
    bytecode: &Bytecode,
    vm: &mut crate::vm::VM,
    symbols: &crate::symbol::SymbolTable,
    cctx: &mut crate::pipeline::CompileCtx,
) -> Result<StoredBytecode, String> {
    use crate::value::ClosureTemplate;
    let (dispatch_wrappers, fn_inline) = cctx.compile_registries_mut();
    let stored_dispatch = dispatch_wrappers.to_stored(symbols);
    let stored_fn_inline = fn_inline.to_stored(symbols);
    let entry = ClosureTemplate {
        bytecode: Rc::new(bytecode.instructions.clone()),
        arity: crate::value::Arity::Exact(0),
        num_locals: 0,
        num_captures: 0,
        num_params: 0,
        constants: Rc::new(bytecode.constants.clone()),
        signal: bytecode.signal,
        capture_params_mask: 0,
        capture_locals_mask: crate::value::CaptureMask::empty(),
        location_map: Rc::new(bytecode.location_map.clone()),
        lir_function: None, // the entry thunk is not JIT'd; closures carry their own LIR
        doc: None,
        syntax: None,
        vararg_kind: crate::hir::VarargKind::List,
        name: None,
        wasm_func_idx: None,
        spirv: std::cell::OnceCell::new(),
        region_table: Vec::new(),
        merged_slots: bytecode.merged_slots.clone(),
        frame_release_slots: bytecode.frame_release_slots.clone(),
        frame_release_regions: bytecode.frame_release_regions.clone(),
        child_protos: Rc::new(bytecode.child_protos.clone()),
    };
    let entry = std::rc::Rc::new(entry);
    let sent =
        crate::value::send::serialize_templates(std::slice::from_ref(&entry), vm.heap(), symbols)?;
    let entry = sent
        .templates
        .into_iter()
        .next()
        .expect("one template in, one template out");
    Ok(StoredBytecode {
        format_version: FORMAT_VERSION,
        entry,
        intern_table: sent.intern_table,
        names: sent.names,
        signal_projection: bytecode.signal_projection.clone(),
        dispatch_wrappers: stored_dispatch,
        fn_inline: stored_fn_inline,
    })
}

/// Rebuild a `Bytecode` from the cache format.
///
/// Symbol ids cross unchanged — an id is its name's hash — and the stored
/// spelling table replays into the loading instance's display memo so the
/// names it now holds can be printed.
pub fn load_bytecode(
    stored: StoredBytecode,
    vm: &mut crate::vm::VM,
    symbols: &mut crate::symbol::SymbolTable,
    cctx: &mut crate::pipeline::CompileCtx,
) -> Result<Bytecode, String> {
    if stored.format_version != FORMAT_VERSION {
        return Err(format!(
            "stdlib cache format mismatch: {} != {}",
            stored.format_version, FORMAT_VERSION
        ));
    }
    let t0 = std::time::Instant::now();
    let mut alloc = crate::primitives::ctx::Alloc::new(vm.heap());
    let mut templates = crate::value::send::deserialize_templates(
        crate::value::send::SendTemplates {
            templates: vec![stored.entry],
            intern_table: stored.intern_table,
            names: stored.names,
        },
        &mut alloc,
        symbols,
    )?;
    let tracing = crate::trace::compile();
    crate::phase!(tracing, "compile", t0, "stdlib deserialize_templates");
    let t1 = std::time::Instant::now();
    let entry = templates
        .pop()
        .expect("deserialize_templates returns one per input");
    crate::phase!(tracing, "compile", t1, "stdlib pop+extract");
    // Restore the cross-unit registries the skipped stdlib compile would have
    // populated.
    let (dispatch_wrappers, fn_inline) = cctx.compile_registries_mut();
    dispatch_wrappers.restore(stored.dispatch_wrappers, symbols);
    fn_inline.restore(stored.fn_inline, symbols);
    Ok(Bytecode {
        instructions: (*entry.bytecode).clone(),
        constants: (*entry.constants).clone(),
        location_map: (*entry.location_map).clone(),
        signal: entry.signal,
        signal_projection: stored.signal_projection,
        child_protos: (*entry.child_protos).clone(),
        merged_slots: entry.merged_slots.clone(),
        frame_release_slots: entry.frame_release_slots.clone(),
        frame_release_regions: entry.frame_release_regions.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let entries = |p: &std::path::Path| -> usize {
            std::fs::read_dir(p).expect("read cache dir").count()
        };
        for entry in std::fs::read_dir(dir.path()).expect("read cache dir") {
            std::fs::remove_file(entry.expect("entry").path()).expect("clear");
        }
        assert_eq!(entries(dir.path()), 0, "cleared");

        let (vm, symbols, cctx) = rt.parts();
        let result =
            compile_file_repl("(sys/join (sys/spawn (fn [] 1)))", symbols, cctx, "<spawn>")
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

    /// `ClosureTemplate.syntax` does not cross the cache, and `(meta/origin f)`
    /// — its only reader — reports a closure's source location from it. So a
    /// stdlib closure has an origin on the compiled path and none on the cached
    /// one. Nothing in the tree depends on that; it is pinned here rather than
    /// left to be rediscovered as a surprise.
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
            "a compiled stdlib closure carries its syntax, so it has an origin"
        );
        drop(compiled);

        let mut hit = Runtime::with_stdlib_cache(cache);
        assert_eq!(hit.stdlib_source(), StdlibSource::Cache);
        assert!(
            origin_is_nil(&mut hit, "(meta/origin map)"),
            "the cache does not carry syntax, so a restored closure has no \
             origin — a known difference between the two paths"
        );
        assert!(
            !origin_is_nil(&mut hit, "(meta/origin (fn [] nil))"),
            "the loss must stay with the restored values: a closure this \
             runtime compiled itself still knows where it came from"
        );
    }
}
