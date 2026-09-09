// audited: 2026-09-09
//! Cache the compiled standard library on disk, so a later process
//! deserializes it instead of running the front end again.
//! docs/impl/stdlib-cache.md

use crate::compiler::Bytecode;
use crate::signals::Signal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
/// The `Bytecode` rides in `entry`, wrapped as a synthetic `ClosureTemplate` so
/// the send module's template path carries the whole cyclic closure graph
/// (docs/impl/stdlib-cache.md). Every other field is one a `ClosureTemplate`
/// has nowhere to hold.
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
    // The binary itself, not its version string: two builds of one version
    // compile stdlib differently the moment a pass changes
    // (docs/impl/stdlib-cache.md).
    build_identity()?.hash(&mut hasher);
    FORMAT_VERSION.hash(&mut hasher);
    // A serialized native-fn immediate carries a `prim_id`, valid only against
    // the table that minted it; without this a foreign id reaches
    // `panic!("unknown prim id")` instead of a miss.
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
            // Never write `path` directly: another process starting right now
            // would read a half-written file (docs/impl/stdlib-cache.md).
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

/// Remove every cache file in `dir` except `keep` — the megabytes each earlier
/// build's key orphans (docs/impl/stdlib-cache.md).
///
/// Call this after the rename, never before: a store that fails must leave the
/// directory as it found it, still holding a file some other process may be
/// about to read. A removal that fails is ignored; it is disk hygiene, not
/// correctness, and the next store tries again.
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
/// The `Bytecode` becomes a synthetic entry `ClosureTemplate` of arity
/// `Exact(0)` — it runs as a thunk — and goes through the send module's
/// template path, which carries the entry pool, the nested-lambda blueprints,
/// their LIR and the region-release tables uniformly.
pub fn store_bytecode(
    bytecode: &Bytecode,
    vm: &mut crate::vm::VM,
    symbols: &crate::symbol::SymbolTable,
    cctx: &mut crate::pipeline::CompileCtx,
) -> Result<StoredBytecode, String> {
    let (dispatch_wrappers, fn_inline) = cctx.compile_registries_mut();
    let stored_dispatch = dispatch_wrappers.to_stored(symbols);
    let stored_fn_inline = fn_inline.to_stored(symbols);
    // The entry thunk is not JIT'd; the nested lambdas carry their own LIR.
    let entry = std::rc::Rc::new(bytecode.clone().into_proto());
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
    // The templates this rebuilds are held Rust-side until the instance is
    // gone, so nothing releases their region by value; it is a process root
    // (docs/impl/region/ctx.md).
    let mut alloc = crate::primitives::ctx::Alloc::process_root(vm.heap());
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
    let entry = std::rc::Rc::try_unwrap(entry).unwrap_or_else(|rc| {
        // One blueprint in, one out — a second holder would mean the intern
        // table kept a reference, which the entry thunk never enters.
        crate::value::TemplateProto {
            location_map: rc.location_map.clone(),
            child_protos: rc.child_protos.clone(),
            merged_slots: rc.merged_slots.clone(),
            frame_release_slots: rc.frame_release_slots.clone(),
            frame_release_regions: rc.frame_release_regions.clone(),
            signal: rc.signal,
            ..crate::value::TemplateProto::new(rc.bytecode.clone(), rc.arity, rc.constants.clone())
        }
    });
    Ok(Bytecode {
        instructions: entry.bytecode,
        constants: entry.constants,
        location_map: entry.location_map,
        signal: entry.signal,
        signal_projection: stored.signal_projection,
        child_protos: entry.child_protos,
        merged_slots: entry.merged_slots,
        frame_release_slots: entry.frame_release_slots,
        frame_release_regions: entry.frame_release_regions,
    })
}

#[cfg(test)]
mod tests;
