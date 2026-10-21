// audited: 2026-10-21
//! The `VM`'s accessors: the allocations it points at, the loads in progress, and the frame it runs.
//!
//! docs/impl/vm.md

use super::*;

impl VM {
    /// Access the root heap.
    #[inline]
    pub fn heap(&mut self) -> &mut crate::value::fiberheap::FiberHeap {
        unsafe { &mut *self.heap_ptr }
    }

    /// The kill switch this VM's charge sites read.
    pub fn kill_switch(&self) -> &crate::value::fiber::KillSwitch {
        &self.kill
    }

    /// Read this VM's charges off `switch` from now on: a worker takes the
    /// switch its spawner holds the other end of. Construction-time only.
    pub(crate) fn set_kill_switch(&mut self, switch: crate::value::fiber::KillSwitch) {
        self.kill_word = switch.word();
        self.kill = switch;
    }

    /// The FFI subsystem this VM owns — its loaded libraries, signatures, and
    /// the error a callback left behind.
    pub(crate) fn ffi(&self) -> &FFISubsystem {
        &self.ffi
    }

    pub(crate) fn ffi_mut(&mut self) -> &mut FFISubsystem {
        &mut self.ffi
    }

    /// Point this VM at its owning instance's compile context. Set by
    /// `RuntimeCore` (the boxed `CompileCtx` has a stable address). See
    /// `compile_ctx_ptr`.
    pub fn set_compile_ctx(&mut self, ctx: *mut crate::pipeline::CompileCtx) {
        self.compile_ctx_ptr = ctx;
    }

    /// Point this VM at its owning instance's symbol table. Set by `RuntimeCore`
    /// (the boxed `SymbolTable` has a stable address). See `symbols_ptr`.
    pub fn set_symbols(&mut self, symbols: *mut crate::symbol::SymbolTable) {
        self.symbols_ptr = symbols;
    }

    /// The owning instance's symbol table, for runtime name interning/resolution.
    /// Returns `None` for a bare VM (no `RuntimeCore`); the readers treat that as
    /// "symbol table unavailable". The borrow is sound by the same contract as
    /// `heap()`/`compile_ctx()`. The `SymbolTable` is a disjoint allocation owned
    /// by the `RuntimeCore` that outlives the VM, reborrowed per call, and the VM
    /// does not touch it during the synchronous read.
    #[allow(clippy::mut_from_ref)]
    pub(crate) fn symbols(&self) -> Option<&mut crate::symbol::SymbolTable> {
        if self.symbols_ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *self.symbols_ptr })
        }
    }

    /// The spelling of a keyword value, through this instance's memo and the
    /// static vocabulary. `None` if `v` is not a keyword or the spelling was
    /// never learned.
    pub(crate) fn keyword_spelling(&self, v: crate::value::Value) -> Option<String> {
        let hash = v.keyword_hash()?;
        crate::value::keyword::resolve_keyword_name(self.symbols().map(|s| &*s), hash)
            .map(str::to_string)
    }

    /// The owning instance's compile context, for the runtime `eval`
    /// instruction. Returns `None` for a bare VM (no `RuntimeCore`), in which
    /// case `(eval …)` of macro-using code is unsupported. The borrow is sound
    /// by the same contract as `heap()`: the `CompileCtx` is a disjoint
    /// allocation owned by the `RuntimeCore` that outlives the VM.
    #[allow(clippy::mut_from_ref)]
    pub(crate) fn compile_ctx(&self) -> Option<&mut crate::pipeline::CompileCtx> {
        if self.compile_ctx_ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *self.compile_ctx_ptr })
        }
    }

    /// Take the reason of the most recent uncaught `:gated` error, if the last
    /// failed execution terminated via a loud gate. Consumes it (returns None on
    /// a second call). The top-level driver uses this to exit 0 with a skip
    /// notice instead of reporting a failure. See `gated_exit_reason`.
    pub fn take_gated_exit_reason(&mut self) -> Option<String> {
        self.gated_exit_reason.take()
    }

    /// The cycle a load of `module_path` would close: every load in progress
    /// from the one of `module_path` on, then `module_path` again, joined by
    /// ` -> `. `None` when no load of it is in progress.
    pub fn loading_cycle(&self, module_path: &str) -> Option<String> {
        let start = self.loading_modules.iter().position(|p| p == module_path)?;
        let mut chain: Vec<&str> = self.loading_modules[start..]
            .iter()
            .map(String::as_str)
            .collect();
        chain.push(module_path);
        Some(chain.join(" -> "))
    }

    /// Push a load of `module_path` onto the loads in progress.
    pub fn mark_module_loading(&mut self, module_path: String) {
        self.loading_modules.push(module_path);
    }

    /// Pop the newest load of `module_path` off the loads in progress.
    pub fn unmark_module_loading(&mut self, module_path: &str) {
        if let Some(i) = self.loading_modules.iter().rposition(|p| p == module_path) {
            self.loading_modules.remove(i);
        }
    }

    /// Get the frame base for the current call frame
    /// Returns 0 if no call frame (top-level execution)
    pub fn current_frame_base(&self) -> usize {
        self.fiber
            .call_stack
            .last()
            .map(|f| f.frame_base)
            .unwrap_or(0)
    }

    /// The operand depth the last root body left at its exit. The stack
    /// `execute_code` hands back cannot show it, because the body ran on a
    /// stack of its own (docs/impl/vm.md).
    pub fn root_exit_depth(&self) -> usize {
        self.root_exit_depth
    }

    /// Push a synthetic trace frame for `name`, whose call site is at `ip` in a
    /// code object carrying `location_map`. Both code objects are real payloads
    /// on this VM's heap, so a trace test exercises the same path a real call
    /// takes.
    #[cfg(test)]
    pub(super) fn push_call_frame(
        &mut self,
        name: &str,
        ip: usize,
        location_map: crate::error::LocationMap,
    ) {
        use crate::value::{Arity, CodeBuilder};
        let heap = self.heap();
        let callee = CodeBuilder::new(Vec::new(), Arity::Exact(0), Vec::new())
            .name(name)
            .build(heap)
            .code();
        let caller = CodeBuilder::new(Vec::new(), Arity::Exact(0), Vec::new())
            .location_map(location_map)
            .build(heap)
            .code();

        let frame_base = self.fiber.stack.len();
        self.fiber.call_depth += 1;
        self.fiber.call_stack.push(crate::value::fiber::CallFrame {
            callee,
            caller,
            ip,
            frame_base,
        });
    }
}
