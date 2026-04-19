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
/// Owns a dedicated thread with a persistent `FiberHeap` for
/// `translate_const` allocations (String/Keyword/Symbol constants).
/// The heap is never freed — constants embedded in native code remain
/// valid for the lifetime of the process.
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
                // Install a persistent fiber heap on this thread.
                // translate_const allocates String/Keyword/Symbol Values
                // in the thread-local heap. Since the heap is never freed,
                // these constants remain valid (kept reachable via
                // JitCode::closure_constants).
                crate::value::fiberheap::install_root_heap();

                while let Ok(task) = task_rx.recv() {
                    let key = task.bytecode_key;
                    let result = match JitCompiler::new() {
                        Ok(compiler) => compiler.compile(
                            &task.lir,
                            task.self_sym,
                            task.symbol_names,
                            Vec::new(),
                        ),
                        Err(e) => Err(e),
                    };
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
pub(crate) fn prepare_task(
    lir: &LirFunction,
    self_sym: Option<SymbolId>,
    symbol_names: HashMap<u32, String>,
    bytecode_key: usize,
) -> JitTask {
    let mut lir = lir.clone();
    lir.syntax = None;
    lir.doc = None;
    JitTask {
        lir,
        self_sym,
        symbol_names,
        bytecode_key,
    }
}
