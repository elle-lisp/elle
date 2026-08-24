//! Disk cache for the standard-library compilation.
//!
//! `init_stdlib` recompiles stdlib.lisp (~2850 lines) on every process start,
//! costing ~2.4s in `compile_file` before execution (5ms) even runs. The whole
//! front end (expand → analyze → regions → lower → emit) is deterministic:
//! same stdlib source, same elle binary → same bytecode. This module turns that
//! work into a one-time cost by serializing the compiled `Bytecode` (plus the
//! per-closure `ClosureTemplate`s and their LIR, so the JIT keeps working) to a
//! content-addressed cache file, keyed by elle version + stdlib source hash +
//! the canonical primitive-table identity (the last because serialized
//! native-fn immediates carry process-local `prim_id`s).
//!
//! Serialization strategy: the cache format is a plain `StoredBytecode` struct
//! that is 100% owned data — no `Rc`, no pointers, no symbol-table ids. Symbols
//! and keywords are carried *by name* (their ids are per-process), and are
//! re-interned on load. `Value`s that appear in the constant pool are scalars
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
const FORMAT_VERSION: u32 = 1;

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
    /// pool, `child_protos` → nested lambdas. Symbols by name; LIR preserved.
    pub entry: crate::value::send::SendableClosure,
    /// Intern table of closure constants reachable from the entry's pool and
    /// its child templates, referenced by `Ref(idx)`.
    pub intern_table: Vec<crate::value::send::SendableClosure>,
    pub signal_projection: Option<HashMap<String, Signal>>,
    /// Cross-unit dispatch-wrapper registry (stdlib `push`/`put`/`add`
    /// monomorphization), snapshotted because the disk cache skips the stdlib
    /// compile that would otherwise populate it.
    pub(crate) dispatch_wrappers: crate::hir::typeinfer::StoredDispatchRegistry,
    /// Cross-unit inline-fn registry (stdlib `inc`/`dec`/… HOF-argument
    /// inlining), likewise snapshotted. Templates whose body contains a `let`
    /// are omitted (their clone needs the defining arena); a performance-only
    /// difference on the cached path.
    pub(crate) fn_inline: crate::hir::typeinfer::StoredFnInlineRegistry,
}
/// Cache directory, honoring `ELLE_CACHE_DIR` override (used in tests).
fn cache_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("ELLE_CACHE_DIR") {
        return std::path::PathBuf::from(dir);
    }
    let base = std::env::var("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|h| std::path::PathBuf::from(h).join(".cache"))
                .unwrap_or_else(|_| std::path::PathBuf::from(".cache"))
        });
    base.join("elle").join("stdlib-cache")
}

/// Content hash of the stdlib source + elle version + primitive-table
/// identity, forming the cache key.
fn cache_key(stdlib_source: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    stdlib_source.hash(&mut hasher);
    // Include the binary/format identity so a changed elle (e.g. a compiler
    // change that alters emitted bytecode) naturally invalidates the cache.
    std::env::var("ELLE_CACHE_VERSION")
        .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string())
        .hash(&mut hasher);
    FORMAT_VERSION.hash(&mut hasher);
    // A serialized native-fn immediate carries a `prim_id`, which is only
    // valid against the exact primitive table that minted it. Mix the table
    // identity in so a prim addition/removal/reorder (a different elle
    // binary) invalidates the cache instead of deserializing a foreign id
    // into `panic!("unknown prim id")`.
    crate::primitives::registration::hash_prim_table_identity(&mut hasher);
    format!("{:016x}.bin", hasher.finish())
}

/// Try to load the compiled stdlib from the disk cache.
///
/// Returns `None` when no cache is enabled or the file is absent; `Some(Err)`
/// when the cache file exists but fails to parse (corrupt / version drift) —
/// the caller recompiles, and the next store overwrites the bad file.
pub fn try_load(
    stdlib_source: &str,
    vm: &mut crate::vm::VM,
    symbols: &mut crate::symbol::SymbolTable,
    cctx: &mut crate::pipeline::CompileCtx,
) -> Option<Result<Bytecode, String>> {
    let path = cache_dir().join(cache_key(stdlib_source));
    let bytes = std::fs::read(&path).ok()?;
    let stored: StoredBytecode = match bincode::deserialize(&bytes) {
        Ok(s) => s,
        Err(e) => return Some(Err(format!("cache decode: {e}"))),
    };
    Some(load_bytecode(stored, vm, symbols, cctx))
}

/// Store the compiled stdlib to the disk cache. Failures are ignored — the
/// cache is an optimization; a fresh compile is always valid.
pub fn try_store(
    stdlib_source: &str,
    bytecode: &Bytecode,
    vm: &mut crate::vm::VM,
    symbols: &crate::symbol::SymbolTable,
    cctx: &mut crate::pipeline::CompileCtx,
) {
    let dir = cache_dir();
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
            let path = dir.join(cache_key(stdlib_source));
            if let Err(e) = std::fs::write(&path, bytes) {
                eprintln!("[stdlib-cache] write failed: {e}");
            }
        }
        Err(e) => eprintln!("[stdlib-cache] serialize failed: {e}"),
    }
}

/// Serialize compiled stdlib bytecode into the cache format.
///
/// The entry constant pool is scalar by construction; the nested-lambda
/// blueprints (`child_protos`) may contain live closure constants, LIR, and
/// region-release tables, so they go through the send module's template
/// serialization (which deep-copies closures and interns them by pointer).
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
        symbol_names: Rc::new(bytecode.symbol_names.clone()),
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
    let (templates, intern_table) =
        crate::value::send::serialize_templates(std::slice::from_ref(&entry), vm.heap(), symbols)?;
    let entry = templates.into_iter().next().expect("one template");
    Ok(StoredBytecode {
        format_version: FORMAT_VERSION,
        entry,
        intern_table,
        signal_projection: bytecode.signal_projection.clone(),
        dispatch_wrappers: stored_dispatch,
        fn_inline: stored_fn_inline,
    })
}

/// Rebuild a `Bytecode` from the cache format.
///
/// Every symbol is re-interned by name into the loading process's table, and
/// the LIR embedded in the templates has its symbol ids remapped to the new
/// table (the JIT materializes `LirConst::Symbol` ids directly).
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
        vec![stored.entry],
        stored.intern_table,
        &mut alloc,
        symbols,
    )?;
    if std::env::var("ELLE_PROFILE").is_ok() {
        eprintln!("[stdlib-cache] deserialize_templates: {:?}", t0.elapsed());
    }
    let t1 = std::time::Instant::now();
    let entry = templates
        .pop()
        .expect("deserialize_templates returns one per input");
    if std::env::var("ELLE_PROFILE").is_ok() {
        eprintln!("[stdlib-cache] pop+extract: {:?}", t1.elapsed());
    }
    // Restore the cross-unit registries the skipped stdlib compile would have
    // populated.
    let (dispatch_wrappers, fn_inline) = cctx.compile_registries_mut();
    dispatch_wrappers.restore(stored.dispatch_wrappers, symbols);
    fn_inline.restore(stored.fn_inline, symbols);
    Ok(Bytecode {
        instructions: (*entry.bytecode).clone(),
        constants: (*entry.constants).clone(),
        symbol_names: (*entry.symbol_names).clone(),
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
    use crate::runtime::Runtime;

    /// Compile a snippet through the full pipeline, then assert that
    /// store→load round-trips to an equivalent `Bytecode` (equal instructions
    /// and constants, closures rebuilt, LIR preserved).
    #[test]
    fn bytecode_roundtrip_preserves_lir_and_closures() {
        let _dir = tempfile::tempdir().expect("tempdir");
        std::env::set_var("ELLE_CACHE_DIR", _dir.path());
        let mut rt = Runtime::new(); // full stdlib: `map` is a stdlib fn
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
            let bytes = bincode::serialize(&stored).expect("serializes");
            let decoded: StoredBytecode = bincode::deserialize(&bytes).expect("deserializes");
            let loaded = load_bytecode(decoded, vm, symbols, cctx).expect("loads");
            assert_eq!(loaded.instructions, bc.instructions, "instructions equal");
            assert_eq!(loaded.signal, bc.signal);
            assert_eq!(loaded.symbol_names.len(), bc.symbol_names.len());
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
            let (vm, symbols, cctx) = rt.parts();
            vm.execute_scheduled(bc, symbols, cctx)
                .expect("runs")
                .as_int()
                .expect("result is an int")
        };
        let mut run = run;
        let r_orig = run(&result);
        let r_loaded = run(&loaded);
        assert_eq!(r_orig, r_loaded, "original and reloaded bytecode agree");
        eprintln!("roundtrip ok: {r_orig} == {r_loaded}");
        std::env::remove_var("ELLE_CACHE_DIR");
    }

    /// Two Runtime instances: the first compiles stdlib and writes the cache,
    /// the second must hit it (cache-load path) and produce working stdlib.
    #[test]
    fn stdlib_cache_hit_produces_working_runtime() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::env::set_var("ELLE_CACHE_DIR", dir.path());
        let mut a = Runtime::new(); // compiles stdlib, writes cache
                                    // The second runtime must hit the disk cache (not recompile) and
                                    // produce a fully working stdlib. Functional check only — timing is
                                    // asserted in the release-mode boot benchmark instead (debug builds
                                    // skew it).
        let mut b = Runtime::new(); // must load from cache
        let probe = |rt: &mut Runtime| {
            use crate::pipeline::compile_file_repl;
            let (vm, symbols, cctx) = rt.parts();
            let src = "(map (fn [x] (* x 2)) (quote (1 2 3)))";
            let result = compile_file_repl(src, symbols, cctx, "<probe>").expect("probe compiles");
            vm.execute_scheduled(&result.0.bytecode, symbols, cctx)
                .expect("probe runs")
        };
        let _ = probe(&mut a);
        let _ = probe(&mut b);
        std::env::remove_var("ELLE_CACHE_DIR");
    }
}

// =============================================================================
// SendValue / TableKey serde for the stdlib cache
// =============================================================================
//
// `SendValue` is the owned deep-copy form the send module produces for
// cross-thread transport, and it is exactly the shape the stdlib cache needs:
// no Rc, no pointers, symbols by name. We implement serde for it directly
// rather than storing `Value`s, which carry process-local ids/pointers.
//
// Only the pure-data variants are supported; runtime-resource variants
// (channels, ports, FFI descriptors) return an error, which makes the cache
// miss and the caller recompile — safe, never wrong.

/// Symmetric serde mirror for `SendValue`. Both directions go through this
/// enum so the bincode encoding is identical (a hand-written `Serialize` that
/// emitted tuples would not round-trip against a derived `Deserialize`, which
/// expects bincode's enum encoding).
#[derive(serde::Serialize, serde::Deserialize)]
enum Mirror {
    Immediate(crate::value::Value),
    Keyword(String),
    Symbol {
        name: String,
        id: u32,
    },
    String(String),
    Pair(Box<Mirror>, Box<Mirror>, Box<Mirror>),
    Seq(Vec<Mirror>, Box<Mirror>),
    Map(
        std::collections::BTreeMap<crate::value::TableKey, Mirror>,
        Box<Mirror>,
    ),
    Buffer(Vec<u8>, Box<Mirror>),
    Float(f64),
    Closure(Box<crate::value::send::SendableClosure>),
    Ref(usize),
    CaptureCell(Box<Mirror>, Box<Mirror>),
}

impl serde::Serialize for crate::value::send::SendValue {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use crate::value::send::SendValue as SV;
        use serde::ser::Error;
        fn to_mirror(sv: &SV) -> Result<Mirror, String> {
            if let SV::Immediate(v) = sv {
                if v.is_heap() && !v.is_native_fn() {
                    return Err(format!("Immediate heap value {}", v.type_name()));
                }
            }
            Ok(match sv {
                SV::Immediate(v) => Mirror::Immediate(*v),
                SV::Keyword(k) => Mirror::Keyword(k.clone()),
                SV::Symbol { name, id } => Mirror::Symbol {
                    name: name.clone(),
                    id: *id,
                },
                SV::String(st) => Mirror::String(st.clone()),
                SV::Pair(a, b, t) => Mirror::Pair(
                    Box::new(to_mirror(a)?),
                    Box::new(to_mirror(b)?),
                    Box::new(to_mirror(t)?),
                ),
                SV::Array(v, t) | SV::Tuple(v, t) | SV::LSet(v, t) | SV::LSetMut(v, t) => {
                    Mirror::Seq(
                        v.iter().map(to_mirror).collect::<Result<_, _>>()?,
                        Box::new(to_mirror(t)?),
                    )
                }
                SV::Struct(m, t) | SV::StructMut(m, t) => Mirror::Map(
                    m.iter()
                        .map(|(k, v)| Ok((k.clone(), to_mirror(v)?)))
                        .collect::<Result<_, String>>()?,
                    Box::new(to_mirror(t)?),
                ),
                SV::Buffer(v, t) | SV::Bytes(v, t) | SV::Blob(v, t) => {
                    Mirror::Buffer(v.clone(), Box::new(to_mirror(t)?))
                }
                SV::LBox(..) => return Err("LBox not needed by stdlib cache".into()),
                SV::CaptureCell(a, b) => {
                    Mirror::CaptureCell(Box::new(to_mirror(a)?), Box::new(to_mirror(b)?))
                }
                SV::Float(f) => Mirror::Float(*f),
                SV::Closure(c) => Mirror::Closure(c.clone()),
                SV::Ref(r) => Mirror::Ref(*r),
                _ => return Err("SendValue variant not serializable by stdlib cache".into()),
            })
        }
        to_mirror(self).map_err(Error::custom)?.serialize(s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::value::send::SendValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use crate::value::send::SendValue as SV;
        fn from_mirror(m: Mirror) -> Result<SV, String> {
            Ok(match m {
                Mirror::Immediate(v) => SV::Immediate(v),
                Mirror::Keyword(k) => SV::Keyword(k),
                Mirror::Symbol { name, id } => SV::Symbol { name, id },
                Mirror::String(st) => SV::String(st),
                Mirror::Pair(a, b, t) => SV::Pair(
                    Box::new(from_mirror(*a)?),
                    Box::new(from_mirror(*b)?),
                    Box::new(from_mirror(*t)?),
                ),
                Mirror::Seq(v, t) => {
                    let vals = v
                        .into_iter()
                        .map(from_mirror)
                        .collect::<Result<Vec<_>, _>>()?;
                    SV::Array(vals, Box::new(from_mirror(*t)?))
                }
                Mirror::Map(m, t) => {
                    let map = m
                        .into_iter()
                        .map(|(k, v)| Ok((k, from_mirror(v)?)))
                        .collect::<Result<_, String>>()?;
                    SV::Struct(map, Box::new(from_mirror(*t)?))
                }
                Mirror::Buffer(v, t) => SV::Bytes(v, Box::new(from_mirror(*t)?)),
                Mirror::Float(f) => SV::Float(f),
                Mirror::Closure(c) => SV::Closure(c),
                Mirror::Ref(r) => SV::Ref(r),
                Mirror::CaptureCell(a, b) => {
                    SV::CaptureCell(Box::new(from_mirror(*a)?), Box::new(from_mirror(*b)?))
                }
            })
        }
        from_mirror(Mirror::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

impl serde::Serialize for crate::value::TableKey {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use crate::value::TableKey as TK;
        match self {
            TK::Nil => (0u8, ()).serialize(s),
            TK::Bool(b) => (1u8, b).serialize(s),
            TK::Int(i) => (2u8, i).serialize(s),
            TK::Symbol(sym) => (3u8, sym.0).serialize(s),
            TK::String(st) => (4u8, st).serialize(s),
            TK::Keyword(k) => (5u8, k).serialize(s),
            TK::EmptyList => (6u8, ()).serialize(s),
            TK::Array(a) => (7u8, a).serialize(s),
            TK::Heap(_) => Err(serde::ser::Error::custom(
                "TableKey::Heap not serializable by stdlib cache",
            )),
        }
    }
}

impl<'de> serde::Deserialize<'de> for crate::value::TableKey {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use crate::value::TableKey as TK;
        #[derive(serde::Deserialize)]
        enum Raw {
            A(u8, ()),
            B(u8, bool),
            C(u8, i64),
            D(u8, u32),
            E(u8, String),
            F(u8, Vec<TK>),
        }
        match Raw::deserialize(d)? {
            Raw::A(0, _) => Ok(TK::Nil),
            Raw::B(1, b) => Ok(TK::Bool(b)),
            Raw::C(2, i) => Ok(TK::Int(i)),
            Raw::D(3, s) => Ok(TK::Symbol(crate::value::SymbolId(s))),
            Raw::E(4, st) => Ok(TK::String(st)),
            Raw::E(5, k) => Ok(TK::Keyword(k)),
            Raw::A(6, _) => Ok(TK::EmptyList),
            Raw::F(7, a) => Ok(TK::Array(a)),
            _ => Err(serde::de::Error::custom("unsupported TableKey variant")),
        }
    }
}
