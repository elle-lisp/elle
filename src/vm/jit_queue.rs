// audited: 2026-11-24
//! The JIT's background queue: a compile submitted and pinned, its result taken and counted, and the drain and settle that end the queue.
//!
//! docs/impl/jit.md
//! docs/config.md
//!
//! `profile_jit_candidate` (jit_entry.rs) submits through here, and every
//! result the worker delivers, or the synchronous compile `--trace=syncjit`
//! runs, comes back through `take_jit_result` or `record_jit_failure`.

use crate::jit::JitRejectionInfo;
use crate::value::CodePin;
use std::sync::Arc;

use super::core::VM;

impl VM {
    /// Record that a compile for `template` is in flight on the worker. The
    /// entry pins `template`'s code region until the result installs, so the
    /// key the worker echoes back still names this function.
    pub(crate) fn record_jit_pending(&mut self, template: crate::value::ClosureTemplate) {
        let pin = CodePin::of(self.heap(), &template);
        self.jit_pending.insert(pin.key() as usize, pin);
    }

    /// Take every result the background worker has finished, without waiting.
    pub(crate) fn poll_jit_completions(&mut self) {
        let results: Vec<_> = match self.jit_worker.as_ref() {
            Some(w) => w.poll().collect(),
            None => return,
        };
        for result in results {
            self.take_jit_result(result);
        }
    }

    /// The compiles this VM has taken, read once it has taken every result the
    /// worker has finished; it waits for none. That makes it a poll, which the
    /// rig's `jit/stats` alone runs (rig/overview.md).
    pub fn poll_jit_stats(&mut self) -> crate::jit::JitStats {
        self.poll_jit_completions();
        self.jit_stats
    }

    /// Take one result from the worker: count its compile, then install its
    /// code or record its failure.
    fn take_jit_result(&mut self, result: crate::jit::worker::JitResult) {
        self.jit_stats.take(result.cost, &result.result);
        // The pin recorded at submit is what keeps `bytecode_key` naming this
        // function; without it the address may already belong to a different
        // function's bytecode, so the result must not be installed under it.
        let pin = self.jit_pending.remove(&result.bytecode_key);
        match result.result {
            Ok(jit_code) => {
                let Some(pin) = pin else { return };
                if self
                    .runtime_config
                    .has_trace_bit(crate::config::trace_bits::JIT)
                {
                    eprintln!(
                        "[jit] background compiled: bc_ptr={:#x}",
                        result.bytecode_key,
                    );
                }
                self.install_pinned_jit_code(pin, Arc::new(jit_code));
            }
            Err(e) => {
                self.record_jit_failure(result.bytecode_key as *const u8, result.name, e, pin)
            }
        }
    }

    /// Record a compile that failed, and name the failures nobody planned for.
    ///
    /// Every failure enters the negative cache, so the function is never
    /// re-submitted whichever kind it was. A refusal the translator plans for
    /// says nothing further; a Cranelift failure or an invalid-LIR result is a
    /// defect in the compiler and prints one line on stderr
    /// (docs/impl/jit.md). Both the worker's results and
    /// the synchronous compile come through here, so a flag meant to show a
    /// codegen failure cannot be the one place that swallows it.
    ///
    /// `name` is the name the compiled LIR carries. `pin` holds the code
    /// region the key was derived from, and is `None` only where the
    /// submission's pin was already lost.
    fn record_jit_failure(
        &mut self,
        bytecode_ptr: *const u8,
        name: Option<String>,
        error: crate::jit::JitError,
        pin: Option<CodePin>,
    ) {
        if !matches!(error, crate::jit::JitError::UnsupportedInstruction(_)) {
            eprintln!("[jit] compilation failed: {}", error);
        }
        self.jit_rejections
            .entry(bytecode_ptr)
            .and_modify(|info| info.attempts += 1)
            .or_insert_with(|| JitRejectionInfo::new(name, error, pin));
    }

    /// Submit a background JIT compilation task for a hot function.
    pub(super) fn submit_jit_task(
        &mut self,
        lir_func: &crate::lir::LirView<'_>,
        closure: &crate::value::Closure,
        bytecode_ptr: *const u8,
    ) {
        let label = closure.template.display_label();
        let task = crate::jit::worker::prepare_task(
            lir_func,
            crate::jit::ChildTable::of(&closure.template),
            bytecode_ptr as usize,
            Some(&label),
        );

        // `--trace=syncjit`: compile here on the VM thread and install
        // immediately; the `elle-jit` worker never spawns. Codegen inputs are
        // identical to the background path (same prepare_task output), so a
        // failure that persists under syncjit indicts codegen or its inputs,
        // while one that vanishes lives at the worker boundary — the task
        // crossing to the worker, or a poll/install racing execution. Diagnosing a
        // suspected JIT race starts here; `--trace=jit,syncjit` logs each
        // synchronous install like the background path logs its own.
        if crate::config::get().has_trace("syncjit") {
            let pin = CodePin::of(self.heap(), &closure.template);
            let (res, cost) = crate::jit::CompileCost::measure(|| {
                crate::jit::JitCompiler::for_process()
                    .and_then(|c| c.compile(&task.lir.view(), task.children.view()))
            });
            self.jit_stats.take(cost, &res);
            match res {
                Ok(jit_code) => {
                    if self
                        .runtime_config
                        .has_trace_bit(crate::config::trace_bits::JIT)
                    {
                        eprintln!(
                            "[jit] synchronous compile (syncjit): bc_ptr={:#x}",
                            bytecode_ptr as usize,
                        );
                    }
                    self.install_pinned_jit_code(pin, Arc::new(jit_code));
                }
                Err(e) => self.record_jit_failure(bytecode_ptr, task.name(), e, Some(pin)),
            }
            return;
        }

        // Lazily spawn the worker thread on first use
        let worker = self
            .jit_worker
            .get_or_insert_with(crate::jit::worker::JitWorker::new);

        if worker.submit(task) {
            self.record_jit_pending((*closure.template).clone());
            if self
                .runtime_config
                .has_trace_bit(crate::config::trace_bits::JIT)
            {
                eprintln!(
                    "[jit] submitted background compilation: label={} bc_ptr={:#x} bclen={}",
                    closure.template.display_label(),
                    bytecode_ptr as usize,
                    closure.template.bytecode().len(),
                );
            }
        }
    }

    /// Block until all pending background JIT compilations complete.
    /// `jit/rejections` calls it so every result is in before it reports;
    /// `--dump=stats` settles the queue instead (`settle_jit_queue`).
    pub fn drain_jit_pending(&mut self) {
        while !self.jit_pending.is_empty() {
            // None once the worker is gone, or its thread has exited.
            let Some(result) = self.jit_worker.as_ref().and_then(|w| w.recv_blocking()) else {
                break;
            };
            self.take_jit_result(result);
        }
    }

    /// Take every result the worker has finished, then drop the worker, and
    /// answer how many compiles were still pending. The drop discards the
    /// queue and stops the compile in progress, as a VM's drop does, so
    /// `--dump=stats` waits for no compile (docs/config.md). A dropped compile
    /// releases its pin and counts nowhere in `jit_stats`, as one the queue
    /// abandons counts nowhere (docs/impl/jit.md).
    pub fn settle_jit_queue(&mut self) -> usize {
        self.poll_jit_completions();
        self.jit_worker = None;
        let dropped = self.jit_pending.len();
        self.jit_pending.clear();
        dropped
    }
}
