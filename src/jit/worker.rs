// audited: 2026-11-26
// docs/impl/jit.md
//! The background JIT worker: the thread Cranelift runs on, and the task and
//! result that cross to it.
//!
//! Compilation moves off the event loop so the interpreter keeps running a hot
//! function while its native code is generated. When compilation finishes, the
//! next call picks the code up from the cache.

use crate::jit::compiler::Abandon;
use crate::jit::{ChildTable, CompileCost, JitCode, JitCompiler, JitError, OwnedChildTable};
use crate::lir::{LirOwned, LirView};
use crate::value::ClosureTemplate;

/// Compilation request sent to the background JIT thread. Plain data, so it is
/// `Send` by its type: the frozen LIR is records in `Vec`s, and its values and
/// its child headers are two words each, which the JIT reads as i64
/// immediates and never dereferences.
pub(crate) struct JitTask {
    /// A copy of the function's frozen LIR.
    pub lir: LirOwned,
    /// A copy of its code payload's child table (docs/impl/jit-closure.md).
    pub children: OwnedChildTable,
    /// Cache key — the bytecode pointer address, cast to usize.
    pub bytecode_key: usize,
    /// What the compile records in the code-address registry, when the task
    /// came from a code object (docs/impl/jit-map.md).
    pub label: Option<String>,
}

impl JitTask {
    /// The name the task's LIR carries, which a rejection reports.
    pub fn name(&self) -> Option<String> {
        self.lir.view().name().map(String::from)
    }

    /// Compile the task with `compiler`, under the task's label.
    pub fn compile(&self, compiler: JitCompiler) -> Result<JitCode, JitError> {
        let compiler = match &self.label {
            Some(label) => compiler.labeled(label.clone()),
            None => compiler,
        };
        compiler.compile(&self.lir.view(), self.children.view())
    }
}

/// Compilation result received from the background JIT thread.
pub(crate) struct JitResult {
    pub bytecode_key: usize,
    /// The task's `JitTask::name`.
    pub name: Option<String>,
    pub result: Result<JitCode, JitError>,
    /// What the compile cost the worker's thread.
    pub cost: CompileCost,
}

// Safety: JitCode is already Send + Sync. JitError is Clone + Debug
// with only owned String fields.
unsafe impl Send for JitResult {}

/// Background JIT compilation worker.
///
/// Owns a dedicated thread that Cranelift-compiles LIR to native code.
/// Compilation allocates no Elle values: a string literal arrives as a
/// `MaterializeConst` template, which is plain data, and symbols and keywords
/// are immediates, so the worker needs no heap of its own.
pub(crate) struct JitWorker {
    tx: crossbeam_channel::Sender<JitTask>,
    rx: crossbeam_channel::Receiver<JitResult>,
    /// Set when the owning VM drops this worker. The thread reads it before
    /// each task and the compile reads it as it runs, so once it is set the
    /// thread delivers nothing more.
    abandon: Abandon,
    #[allow(dead_code)]
    handle: std::thread::JoinHandle<()>,
}

impl JitWorker {
    /// Spawn the background JIT compilation thread.
    pub fn new() -> Self {
        let (task_tx, task_rx) = crossbeam_channel::unbounded::<JitTask>();
        let (result_tx, result_rx) = crossbeam_channel::unbounded::<JitResult>();
        let abandon = Abandon::default();
        let owner_gone = abandon.clone();

        let handle = std::thread::Builder::new()
            .name("elle-jit".into())
            .spawn(move || {
                crate::io::sigfd::mask_all_signals_on_this_thread();

                while let Ok(task) = task_rx.recv() {
                    if owner_gone.is_set() {
                        break;
                    }
                    let (result, cost) = CompileCost::measure(|| {
                        JitCompiler::for_process().and_then(|compiler| {
                            task.compile(compiler.abandoned_by(owner_gone.clone()))
                        })
                    });
                    if let Err(JitError::Abandoned) = result {
                        break;
                    }
                    let _ = result_tx.send(JitResult {
                        bytecode_key: task.bytecode_key,
                        name: task.name(),
                        result,
                        cost,
                    });
                }
            })
            .expect("failed to spawn JIT worker thread");

        JitWorker {
            tx: task_tx,
            rx: result_rx,
            abandon,
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

/// The queue and the compile in progress end with the VM that owns them
/// (docs/impl/jit.md). The drop does not join the thread, so the VM does not
/// wait for a compile that is inside Cranelift's backend.
impl Drop for JitWorker {
    fn drop(&mut self) {
        self.abandon.set();
    }
}

/// Prepare a `JitTask` from a frozen function and its child table by copying
/// both out of whatever pages hold them, which the worker's thread cannot read
/// (docs/impl/jit.md).
///
/// `template` is the code object the function came from. It labels the
/// compile (docs/impl/jit-map.md), and its `display_label` backfills a
/// nameless LIR, the common case, so a rejection names the function.
pub(crate) fn prepare_task(
    lir: &LirView<'_>,
    children: ChildTable<'_>,
    bytecode_key: usize,
    template: Option<&ClosureTemplate>,
) -> JitTask {
    let mut lir = lir.to_owned();
    if lir.view().name().is_none() {
        lir.set_name(template.map(ClosureTemplate::display_label));
    }
    JitTask {
        lir,
        children: children.to_owned(),
        bytecode_key,
        label: template.map(crate::jit::registry::label),
    }
}

#[cfg(test)]
mod tests;
