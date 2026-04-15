use crate::error::{LocationMap, StackFrame};
use crate::ffi::FFISubsystem;
use crate::primitives::def::Doc;
use crate::reader::SourceLoc;
use crate::value::fiber::CallFrame;
use crate::value::{
    BytecodeFrame, Closure, Fiber, FiberHandle, SignalBits, SuspendedFrame, Value, SIG_ERROR,
    SIG_FUEL, SIG_HALT, SIG_OK, SIG_SWITCH,
};
use rustc_hash::FxHashMap;
use std::collections::HashMap;
use std::rc::Rc;

use crate::jit::{JitCode, JitRejectionInfo};

pub(crate) struct TailCallInfo {
    pub bytecode: Rc<Vec<u8>>,
    pub constants: Rc<Vec<Value>>,
    pub env: Rc<Vec<Value>>,
    pub location_map: Rc<LocationMap>,
    pub rotation_safe: bool,
    pub squelch_mask: SignalBits,
}

/// Pending fiber resume for the trampoline.
///
/// Set by `handle_fiber_resume_signal` when it wants to switch fibers
/// without recursing. Consumed by the trampoline in `do_fiber_resume`.
pub(crate) struct PendingFiberResume {
    pub handle: FiberHandle,
    pub fiber_value: Value,
}

pub struct VM {
    /// Mutable runtime configuration: trace flags, JIT/WASM policy.
    /// Accessible from Elle via `(vm/config)`.
    pub runtime_config: crate::config::RuntimeConfig,
    /// The current fiber holding all per-execution state:
    /// operand stack, call frames, exception handlers, coroutine state.
    pub fiber: Fiber,
    /// Handle to the current fiber's FiberHandle, if it came from a
    /// `fiber/new` allocation. `None` for the root fiber (which lives
    /// directly on the VM, not behind a handle). Used to wire up
    /// `child.parent` back-pointers during fiber resume.
    pub current_fiber_handle: Option<FiberHandle>,
    /// Cached Value for the current fiber. `None` for the root
    /// fiber. Used to set `child.parent_value` during resume chain wiring,
    /// so `fiber/parent` can return the original Value without re-allocating.
    pub current_fiber_value: Option<Value>,
    pub(crate) ffi: FFISubsystem,
    /// Modules currently being loaded (circular-import guard).
    /// Added before execution, removed after. If a module is in this set
    /// when import-file is called, it's a circular dependency.
    pub loading_modules: std::collections::HashSet<String>,
    /// Plugins already loaded (path → return value). Prevents double-loading
    /// which would re-register primitives and leak library handles.
    pub loaded_plugins: HashMap<String, Value>,
    pub closure_call_counts: FxHashMap<*const u8, usize>,
    pub location_map: LocationMap,
    pub tail_call_env_cache: Vec<Value>,
    pub env_cache: Vec<Value>,
    pub(crate) pending_tail_call: Option<TailCallInfo>,
    pub(crate) pending_fiber_resume: Option<PendingFiberResume>,
    /// Source location of the instruction that produced the current error.
    /// Resolved by the dispatch loop using the current closure's LocationMap.
    /// Reset to None at each translation boundary entry.
    /// Guarded by is_none() — innermost (origin) location wins over outer
    /// call sites. This also protects against fiber error propagation
    /// overwriting the child fiber's error origin.
    pub(crate) error_loc: Option<SourceLoc>,
    /// JIT code cache: bytecode pointer → compiled native code.
    pub jit_cache: FxHashMap<*const u8, Rc<JitCode>>,
    /// Documentation for all named forms (primitives, special forms, macros).
    /// Keyed by name string for direct lookup via `doc` and `vm/primitive-meta`.
    pub docs: HashMap<String, Doc>,
    /// JIT rejection log: bytecode pointer → rejection info.
    /// Records first rejection per closure template. Used by
    /// `(jit/rejections)` primitive and `--stats` CLI flag.
    pub jit_rejections: FxHashMap<*const u8, JitRejectionInfo>,
    /// Cached Expander for runtime `eval`. Avoids re-loading the prelude
    /// on every eval call. Taken out during eval, put back after.
    pub eval_expander: Option<crate::syntax::Expander>,
    /// User-provided command-line arguments, from everything after `--`
    /// in the argv passed to the elle binary. Empty if no `--` was given.
    /// Set by `main.rs` before the file-execution loop. Read by `sys/args`.
    pub user_args: Vec<String>,
    /// The source argument: the script file path, `"-"` for stdin, or `""`
    /// in REPL mode. Set by `main.rs` at the same point as `user_args`.
    /// Read by `sys/argv`. Empty string means REPL mode.
    pub source_arg: String,
    /// Whether JIT compilation is enabled.
    /// Controlled by `--jit=N` CLI flag: `0` disables, `N>0` enables.
    /// Defaults to `true`.
    pub jit_enabled: bool,
    /// JIT hotness threshold: a closure must be called this many times
    /// before it becomes a JIT compilation candidate.
    /// Set by `--jit=N` (threshold = N-1), defaulting to 10.
    pub jit_hotness_threshold: usize,
    /// Lazy WASM compilation tier. When `--wasm=N`, hot closures are
    /// compiled to per-closure WASM modules and dispatched through Wasmtime.
    pub wasm_tier: Option<crate::wasm::lazy::WasmTier>,
    /// Closures that failed WASM compilation (contain MakeClosure, TailCall, etc.)
    pub(crate) wasm_rejections: FxHashMap<*const u8, ()>,
}

/// Create a dummy root closure for the root fiber.
/// The root fiber doesn't execute a closure directly — it's the
/// execution context for top-level bytecode. This closure is never
/// called; it exists only to satisfy Fiber's constructor.
fn root_closure() -> Rc<Closure> {
    use crate::signals::Signal;
    use crate::value::types::Arity;
    use crate::value::ClosureTemplate;
    Rc::new(Closure {
        template: Rc::new(ClosureTemplate {
            bytecode: Rc::new(vec![]),
            arity: Arity::Exact(0),
            num_locals: 0,
            num_captures: 0,
            num_params: 0,
            constants: Rc::new(vec![]),
            signal: Signal::silent(),
            capture_params_mask: 0,
            capture_locals_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            rotation_safe: false,
            lir_function: None,
            doc: None,
            syntax: None,
            vararg_kind: crate::hir::VarargKind::List,
            name: None,
            result_is_immediate: false,
            has_outward_heap_set: false,
            wasm_func_idx: None,
        }),
        env: Rc::new(vec![]),
        squelch_mask: SignalBits::EMPTY,
    })
}

impl VM {
    pub fn new() -> Self {
        // Install the root fiber heap before any allocation can happen.
        crate::value::fiberheap::install_root_heap();

        let mut fiber = Fiber::new(root_closure(), SIG_OK);
        // Root fiber starts alive (it's the currently executing context)
        fiber.status = crate::value::FiberStatus::Alive;

        let rc = crate::config::RuntimeConfig::from_static_config(crate::config::get());
        // Merge --trace= keywords from CLI into the RuntimeConfig
        let mut rc = rc;
        if !crate::config::get().trace_keywords.is_empty() {
            let mut kws = rc.trace.clone();
            for kw in &crate::config::get().trace_keywords {
                kws.insert(kw.clone());
            }
            rc.set_trace(kws);
        }

        let jit_enabled = rc.jit.enabled();
        let jit_threshold = rc.jit.threshold();

        VM {
            runtime_config: rc,
            fiber,
            current_fiber_handle: None, // root fiber has no handle
            current_fiber_value: None,  // root fiber has no Value
            ffi: FFISubsystem::new(),
            loading_modules: std::collections::HashSet::new(),
            loaded_plugins: HashMap::new(),
            closure_call_counts: FxHashMap::default(),
            location_map: LocationMap::new(),
            tail_call_env_cache: Vec::with_capacity(256),
            env_cache: Vec::with_capacity(256),
            pending_tail_call: None,
            pending_fiber_resume: None,
            error_loc: None,
            jit_cache: FxHashMap::default(),
            jit_rejections: FxHashMap::default(),
            docs: HashMap::new(),
            eval_expander: None,
            user_args: Vec::new(),
            source_arg: String::new(),
            jit_enabled,
            jit_hotness_threshold: jit_threshold,
            wasm_tier: if crate::config::get().wasm > 0 && !crate::config::get().wasm_full {
                crate::wasm::lazy::WasmTier::new().ok()
            } else {
                None
            },
            wasm_rejections: FxHashMap::default(),
        }
    }

    /// Reset the VM's fiber and transient state for reuse.
    ///
    /// Preserves: docs, ffi, jit_cache, eval_expander, env_cache,
    /// tail_call_env_cache, fiber heap Box (reused for pointer stability).
    /// Resets: fiber, call state, location map,
    /// loaded modules, closure call counts.
    pub fn reset_fiber(&mut self) {
        // The root heap is persistent (lives in ROOT_HEAP thread-local).
        // Do not clear it — root fiber objects accumulate across resets,
        // so Values returned by execute_bytecode remain valid.
        // self.fiber.heap is an unused Box<FiberHeap>; it is dropped and
        // recreated with the new Fiber, costing one allocation. This is
        // acceptable; making fiber.heap Option<> would add branches everywhere.
        self.fiber = Fiber::new(root_closure(), SIG_OK);
        self.fiber.status = crate::value::FiberStatus::Alive;
        self.current_fiber_handle = None;
        self.current_fiber_value = None;
        self.pending_tail_call = None;
        self.pending_fiber_resume = None;
        self.error_loc = None;
        self.closure_call_counts.clear();
        self.jit_rejections.clear();
        self.location_map = LocationMap::new();
        self.loading_modules.clear();
    }

    /// Set the location map for mapping bytecode instructions to source locations
    pub fn set_location_map(&mut self, map: LocationMap) {
        self.location_map = map;
    }

    /// Get the location map for bytecode instruction lookups
    pub fn get_location_map(&self) -> &LocationMap {
        &self.location_map
    }

    /// Format a runtime error value with source location.
    pub(crate) fn format_error_with_location(&self, err_value: Value) -> String {
        let mut result = String::new();

        // Stack trace first (shallowest frame first, drilling down to error origin)
        let trace = self.capture_stack_trace();
        if !trace.is_empty() {
            const MAX_TRACE_DEPTH: usize = 20;
            for frame in trace.iter().rev().take(MAX_TRACE_DEPTH) {
                if let Some(name) = &frame.function_name {
                    result.push_str(&format!("  in {}", name));
                    if let Some(loc) = &frame.location {
                        result.push_str(&format!(" at {}", loc));
                    }
                    result.push('\n');
                }
            }
            if trace.len() > MAX_TRACE_DEPTH {
                result.push_str(&format!(
                    "  ... {} more frames\n",
                    trace.len() - MAX_TRACE_DEPTH
                ));
            }
        }

        // Error location and source context
        if let Some(loc) = &self.error_loc {
            result.push_str(&format!("  at {}\n", loc));

            // Add source context if available
            if let Some(source) = crate::error::formatting::load_source_for_loc(loc) {
                if let Some(line) = crate::error::formatting::extract_source_line(&source, loc.line)
                {
                    let truncated = if line.len() > 120 {
                        format!("{}...", &line[..117])
                    } else {
                        line.to_string()
                    };
                    result.push_str(&format!("   {}\n", truncated));

                    let caret = crate::error::formatting::highlight_column(&line, loc.col);
                    result.push_str(&format!("   {}\n", caret));
                }
            }
        }

        // Error value last
        result.push_str(&format!("✗ Runtime error: {:?}", err_value));

        result
    }

    /// Record a closure call and return whether it's "hot" (called N+ times,
    /// where N is `jit_hotness_threshold`, default 10, set via `--jit=N`).
    pub fn record_closure_call(&mut self, bytecode_ptr: *const u8) -> bool {
        let count = self.closure_call_counts.entry(bytecode_ptr).or_insert(0);
        *count += 1;
        *count >= self.jit_hotness_threshold
    }

    /// Get call count for a closure
    pub fn get_closure_call_count(&self, bytecode_ptr: *const u8) -> usize {
        self.closure_call_counts
            .get(&bytecode_ptr)
            .copied()
            .unwrap_or(0)
    }

    /// Check if a module is currently being loaded (circular dependency).
    pub fn is_module_loading(&self, module_path: &str) -> bool {
        self.loading_modules.contains(module_path)
    }

    /// Mark a module as currently loading (for circular-import detection).
    pub fn mark_module_loading(&mut self, module_path: String) {
        self.loading_modules.insert(module_path);
    }

    /// Unmark a module as loading (after execution completes).
    pub fn unmark_module_loading(&mut self, module_path: &str) {
        self.loading_modules.remove(module_path);
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

    pub fn push_call_frame(
        &mut self,
        name: String,
        ip: usize,
        location_map: Rc<crate::error::LocationMap>,
    ) {
        let frame_base = self.fiber.stack.len();
        self.fiber.call_depth += 1;
        self.fiber.call_stack.push(CallFrame {
            name: Rc::from(name.as_str()),
            ip,
            frame_base,
            location_map,
        });
    }

    pub fn push_call_frame_with_base(
        &mut self,
        name: String,
        ip: usize,
        frame_base: usize,
        location_map: Rc<crate::error::LocationMap>,
    ) {
        self.fiber.call_depth += 1;
        self.fiber.call_stack.push(CallFrame {
            name: Rc::from(name.as_str()),
            ip,
            frame_base,
            location_map,
        });
    }

    pub fn pop_call_frame(&mut self) {
        if self.fiber.call_depth > 0 {
            self.fiber.call_depth -= 1;
            self.fiber.call_stack.pop();
        }
    }

    pub fn format_stack_trace(&self) -> String {
        if self.fiber.call_stack.is_empty() {
            "No call frames".to_string()
        } else {
            let mut trace = String::new();
            for (i, frame) in self.fiber.call_stack.iter().rev().enumerate() {
                trace.push_str(&format!("  #{}: {} (ip={})\n", i, frame.name, frame.ip));
            }
            trace
        }
    }

    /// Capture current call stack as trace frames
    pub fn capture_stack_trace(&self) -> Vec<StackFrame> {
        self.fiber
            .call_stack
            .iter()
            .rev()
            .map(|frame| {
                let location = frame.location_map.get(&frame.ip).cloned();
                StackFrame {
                    function_name: Some(frame.name.to_string()),
                    location,
                }
            })
            .collect()
    }

    /// Wrap a string error with stack trace information
    pub fn wrap_error(&self, error: String) -> String {
        let trace = self.capture_stack_trace();
        if trace.is_empty() {
            return error;
        }

        let mut result = error;
        for frame in &trace {
            result.push_str("\n    in ");
            if let Some(ref name) = frame.function_name {
                result.push_str(name);
            } else {
                result.push_str("<anonymous>");
            }
            if let Some(ref loc) = frame.location {
                result.push_str(&format!(" at {}", loc));
            }
        }
        result
    }

    #[inline(always)]
    pub fn read_u8(&self, bytecode: &[u8], ip: &mut usize) -> u8 {
        let val = bytecode[*ip];
        *ip += 1;
        val
    }

    #[inline(always)]
    pub fn read_u16(&self, bytecode: &[u8], ip: &mut usize) -> u16 {
        let high = bytecode[*ip] as u16;
        let low = bytecode[*ip + 1] as u16;
        *ip += 2;
        (high << 8) | low
    }

    #[inline(always)]
    pub fn read_i16(&self, bytecode: &[u8], ip: &mut usize) -> i16 {
        self.read_u16(bytecode, ip) as i16
    }

    pub(crate) fn ffi(&self) -> &FFISubsystem {
        &self.ffi
    }

    pub(crate) fn ffi_mut(&mut self) -> &mut FFISubsystem {
        &mut self.ffi
    }

    /// Resume execution from suspended frames.
    ///
    /// Replays the frame chain from innermost (index 0) to outermost
    /// (last index), threading the resume value through. For single-frame
    /// suspension (signal-based), this is equivalent to a simple resume.
    /// For multi-frame suspension (yield-through-calls), this replays the
    /// full call chain.
    ///
    /// Handles two frame types:
    /// - `Bytecode`: restores the saved operand stack and continues bytecode
    ///   execution from the saved instruction pointer.
    /// - `FiberResume`: resumes a suspended sub-fiber (from `defer`/`protect`)
    ///   with the current value via `do_fiber_resume`, using the proper
    ///   fiber-swap machinery so heap context and parent/child chain are correct.
    ///
    /// Returns SignalBits. The result value is stored in `self.fiber.signal`.
    pub fn resume_suspended(
        &mut self,
        frames: Vec<SuspendedFrame>,
        resume_value: Value,
    ) -> SignalBits {
        if frames.is_empty() {
            self.fiber.signal = Some((SIG_OK, resume_value));
            return SIG_OK;
        }

        // Save current stack state
        let saved_stack = std::mem::take(&mut self.fiber.stack);

        let mut current_value = resume_value;

        for i in 0..frames.len() {
            let frame = &frames[i];

            match frame {
                SuspendedFrame::FiberResume {
                    handle,
                    fiber_value,
                } => {
                    // Trampoline: instead of calling do_fiber_resume (which
                    // would recurse on the Rust stack), set pending_fiber_resume
                    // and return SIG_SWITCH. The trampoline in do_fiber_resume
                    // will handle the fiber transition iteratively.
                    handle.with_mut(|f| {
                        f.signal = Some((SIG_OK, current_value));
                    });

                    self.pending_fiber_resume = Some(PendingFiberResume {
                        handle: handle.clone(),
                        fiber_value: *fiber_value,
                    });

                    // Save remaining outer frames for later resumption.
                    if i + 1 < frames.len() {
                        self.fiber.suspended = Some(frames[i + 1..].to_vec());
                    }

                    self.fiber.signal = Some((SIG_SWITCH, Value::NIL));
                    self.fiber.stack = saved_stack;
                    return SIG_SWITCH;
                }

                SuspendedFrame::Bytecode(frame) => {
                    // Restore this frame's stack
                    self.fiber.stack.clear();
                    self.fiber.stack.extend(frame.stack.iter().copied());

                    // For yield frames and caller frames: the resume value is the
                    // "return value" of the suspended operation (yield result, or
                    // call return). Push it so the next instruction sees it.
                    // For fuel/signal-pause frames: the instruction at frame.ip
                    // re-executes from scratch — no extra value is injected.
                    if frame.push_resume_value {
                        self.fiber.stack.push(current_value);
                    }

                    if self
                        .runtime_config
                        .has_trace_bit(crate::config::trace_bits::CALL)
                    {
                        let opcode = if frame.ip < frame.bytecode.len() {
                            frame.bytecode[frame.ip]
                        } else {
                            255
                        };
                        let env_ptr = std::rc::Rc::as_ptr(&frame.env) as usize;
                        eprintln!(
                            "[resume] frame={} ip={} bc_len={} opcode={} saved_stack={} push_rv={} final_stack={} env_len={} env_ptr={:#x} rv_type={}",
                            i, frame.ip, frame.bytecode.len(), opcode,
                            frame.stack.len(), frame.push_resume_value,
                            self.fiber.stack.len(), frame.env.len(),
                            env_ptr, current_value.type_name(),
                        );
                        for (si, sv) in self.fiber.stack.iter().enumerate() {
                            eprintln!("  stack[{}] = {} {:?}", si, sv.type_name(), sv);
                        }
                        // Only dump env for small envs (inner closures, not stdlib)
                        if frame.env.len() <= 5 {
                            for (ei, ev) in frame.env.iter().enumerate() {
                                let detail = if ev.is_capture_cell() {
                                    if let Some(cell_ref) = ev.as_capture_cell() {
                                        let inner = *cell_ref.borrow();
                                        let lbox_ptr = cell_ref as *const _ as usize;
                                        format!(
                                            "box(ptr={:#x}) -> {} {:?}",
                                            lbox_ptr,
                                            inner.type_name(),
                                            inner
                                        )
                                    } else {
                                        format!("{} {:?}", ev.type_name(), ev)
                                    }
                                } else {
                                    format!("{} {:?}", ev.type_name(), ev)
                                };
                                eprintln!("  env[{}] = {}", ei, detail);
                            }
                        }
                    }

                    let exec = self.execute_bytecode_from_ip(
                        &frame.bytecode,
                        &frame.constants,
                        &frame.env,
                        frame.ip,
                        &frame.location_map,
                    );

                    if exec.bits.is_ok() {
                        let (_, v) = self.fiber.signal.take().unwrap();
                        if self
                            .runtime_config
                            .has_trace_bit(crate::config::trace_bits::FIBER)
                        {
                            eprintln!(
                                "[resume_suspended] frame {} OK: val_type={} total_frames={}",
                                i,
                                v.type_name(),
                                frames.len(),
                            );
                        }
                        current_value = v;
                    } else {
                        if self
                            .runtime_config
                            .has_trace_bit(crate::config::trace_bits::FIBER)
                        {
                            let susp_len =
                                self.fiber.suspended.as_ref().map(|v| v.len()).unwrap_or(0);
                            let remaining = frames.len() - i - 1;
                            eprintln!(
                                "[resume_suspended] frame {} non-OK: bits={} susp_frames={} remaining={}",
                                i, exec.bits, susp_len, remaining,
                            );
                        }
                        if !exec.bits.contains(SIG_HALT) && self.fiber.suspended.is_none() {
                            self.fiber.suspended =
                                Some(vec![SuspendedFrame::Bytecode(BytecodeFrame {
                                    bytecode: exec.bytecode,
                                    constants: exec.constants,
                                    env: exec.env,
                                    ip: exec.ip,
                                    stack: exec.stack,
                                    location_map: exec.location_map,
                                    push_resume_value: !exec.bits.contains(SIG_FUEL),
                                })]);
                        }

                        // For suspending signals (any bits except error/halt),
                        // merge remaining outer frames
                        if !exec.bits.contains(SIG_ERROR)
                            && !exec.bits.contains(SIG_HALT)
                            && i + 1 < frames.len()
                        {
                            if let Some(ref mut new_frames) = self.fiber.suspended {
                                for f in frames[i + 1..].iter() {
                                    new_frames.push(f.clone());
                                }
                            }
                        }

                        self.fiber.stack = saved_stack;
                        return exec.bits;
                    }
                }
            }
        }

        self.fiber.stack = saved_stack;
        self.fiber.signal = Some((SIG_OK, current_value));
        SIG_OK
    }
}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_bits() {
        use crate::value::{SIG_ERROR, SIG_OK, SIG_YIELD};

        assert_eq!(SIG_OK.raw(), 0);
        assert_eq!(SIG_ERROR.raw(), 1);
        assert_eq!(SIG_YIELD.raw(), 2);

        let mask = SIG_ERROR | SIG_YIELD;
        assert!(mask.contains(SIG_ERROR));
        assert!(mask.contains(SIG_YIELD));
        assert!(!mask.contains(SIG_OK)); // SIG_OK has no bits, contains() returns false
    }

    #[test]
    fn test_capture_stack_trace() {
        use std::collections::HashMap;
        let mut vm = VM::new();
        let empty_map = Rc::new(HashMap::new());

        vm.push_call_frame("function_a".to_string(), 10, empty_map.clone());
        vm.push_call_frame("function_b".to_string(), 20, empty_map.clone());
        vm.push_call_frame("function_c".to_string(), 30, empty_map.clone());

        let trace = vm.capture_stack_trace();

        assert_eq!(trace.len(), 3);
        assert_eq!(trace[0].function_name, Some("function_c".to_string()));
        assert_eq!(trace[1].function_name, Some("function_b".to_string()));
        assert_eq!(trace[2].function_name, Some("function_a".to_string()));
    }

    #[test]
    fn test_wrap_error_with_trace() {
        use std::collections::HashMap;
        let mut vm = VM::new();
        let empty_map = Rc::new(HashMap::new());

        vm.push_call_frame("outer".to_string(), 5, empty_map.clone());
        vm.push_call_frame("inner".to_string(), 15, empty_map.clone());

        let error_msg = "Something went wrong".to_string();
        let wrapped = vm.wrap_error(error_msg);

        assert!(wrapped.contains("Something went wrong"));
        assert!(wrapped.contains("inner"));
        assert!(wrapped.contains("outer"));
    }

    #[test]
    fn test_wrap_error_empty_stack() {
        let vm = VM::new();

        let error_msg = "Error with no context".to_string();
        let wrapped = vm.wrap_error(error_msg.clone());

        assert_eq!(wrapped, error_msg);
    }
}
