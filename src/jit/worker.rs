//! Background JIT compilation worker thread.
//!
//! Moves Cranelift compilation off the event loop so the interpreter
//! continues running hot functions while native code is generated in
//! the background. When compilation finishes, the next call to the
//! function picks up the compiled code from cache.
//!
//! Modeled on `StdinThread` in `src/io/threadpool.rs`.

use std::collections::HashMap;

use crate::jit::{JitCode, JitCompiler, JitError};
use crate::lir::LirFunction;
use crate::value::SymbolId;
/// Cumulative Cranelift compilation time (ns) and task count across the
/// process, readable by embedders for profiling (`ELLE_PROFILE=1`).
pub static JIT_COMPILE_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static JIT_COMPILE_TASKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Compilation request sent to the background JIT thread.
pub(crate) struct JitTask {
    /// Cloned LIR with syntax/doc stripped. ValueConsts are left intact:
    /// the JIT reads their tag/payload as i64 immediates, never
    /// dereferencing heap pointers during compilation.
    pub lir: LirFunction,
    pub self_sym: Option<SymbolId>,
    pub symbol_names: HashMap<u32, String>,
    /// Cache key — the bytecode pointer address, cast to usize.
    pub bytecode_key: usize,
}

// Safety: LirFunction after stripping syntax (Rc<Syntax>) and doc
// contains only owned data and Value (Copy, two u64 fields). The JIT
// compiler reads Value tag/payload as i64 immediates and never
// dereferences heap pointers during compilation.
unsafe impl Send for JitTask {}

/// Compilation result received from the background JIT thread.
pub(crate) struct JitResult {
    pub bytecode_key: usize,
    pub result: Result<JitCode, JitError>,
}

// Safety: JitCode is already Send + Sync. JitError is Clone + Debug
// with only owned String fields.
unsafe impl Send for JitResult {}

/// Background JIT compilation worker.
///
/// Owns a dedicated thread that Cranelift-compiles LIR to native code.
/// Compilation allocates no Elle values: string constants arrive
/// pre-resolved as `ValueConst`, and symbols/keywords are immediates — so the
/// worker needs no heap of its own.
pub(crate) struct JitWorker {
    tx: crossbeam_channel::Sender<JitTask>,
    rx: crossbeam_channel::Receiver<JitResult>,
    #[allow(dead_code)]
    handle: std::thread::JoinHandle<()>,
}

impl JitWorker {
    /// Spawn the background JIT compilation thread.
    pub fn new() -> Self {
        let (task_tx, task_rx) = crossbeam_channel::unbounded::<JitTask>();
        let (result_tx, result_rx) = crossbeam_channel::unbounded::<JitResult>();

        let handle = std::thread::Builder::new()
            .name("elle-jit".into())
            .spawn(move || {
                crate::io::sigfd::mask_all_signals_on_this_thread();

                while let Ok(task) = task_rx.recv() {
                    let key = task.bytecode_key;
                    let t0 = std::time::Instant::now();
                    let result = match JitCompiler::new() {
                        Ok(compiler) => compiler.compile(
                            &task.lir,
                            task.self_sym,
                            task.symbol_names,
                            Vec::new(),
                        ),
                        Err(e) => Err(e),
                    };
                    JIT_COMPILE_NS.fetch_add(
                        t0.elapsed().as_nanos() as u64,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    JIT_COMPILE_TASKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let _ = result_tx.send(JitResult {
                        bytecode_key: key,
                        result,
                    });
                }
            })
            .expect("failed to spawn JIT worker thread");

        JitWorker {
            tx: task_tx,
            rx: result_rx,
            handle,
        }
    }

    /// Send a compilation task to the background thread.
    /// Returns `true` if sent successfully, `false` if the channel is
    /// disconnected (worker thread panicked).
    pub fn submit(&self, task: JitTask) -> bool {
        self.tx.send(task).is_ok()
    }

    /// Non-blocking poll for completed compilations.
    /// Returns an iterator of all available results.
    pub fn poll(&self) -> impl Iterator<Item = JitResult> + '_ {
        self.rx.try_iter()
    }

    /// Blocking receive: wait for the next result (used to drain
    /// pending compilations for diagnostics like `jit/rejections`).
    /// Returns `None` if the worker thread has exited.
    pub fn recv_blocking(&self) -> Option<JitResult> {
        self.rx.recv().ok()
    }
}

/// Prepare a `JitTask` from a LirFunction by cloning and stripping
/// non-Send fields (syntax, doc).
///
/// `display_name` backfills a nameless LIR (the common case — lowering
/// names few functions) from the closure template, so the compile records
/// a readable entry in the code-address registry
/// (docs/impl/jit.md § "The code-address registry").
pub(crate) fn prepare_task(
    lir: &LirFunction,
    self_sym: Option<SymbolId>,
    symbol_names: HashMap<u32, String>,
    bytecode_key: usize,
    display_name: Option<&str>,
) -> JitTask {
    let mut lir = lir.clone();
    lir.syntax = None;
    lir.doc = None;
    if lir.name.is_none() {
        lir.name = display_name.map(String::from);
    }
    JitTask {
        lir,
        self_sym,
        symbol_names,
        bytecode_key,
    }
}

// A string literal lowers to `MaterializeConst` in every position (value:
// `HirKind::String`; pattern: the materialize-compare-free in
// `lir/lower/pattern.rs`), which the JIT translates via
// `elle_jit_materialize_const` — so no raw `LirConst::String` reaches the
// translator.
