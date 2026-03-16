use crate::error::{LocationMap, StackFrame};
use crate::ffi::FFISubsystem;
use crate::primitives::def::Doc;
use crate::reader::SourceLoc;
use crate::value::fiber::CallFrame;
use crate::value::{
    BytecodeFrame, Closure, Fiber, FiberHandle, SignalBits, SuspendedFrame, Value, SIG_ERROR,
    SIG_HALT, SIG_OK,
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
}

pub struct VM {
    /// The current fiber holding all per-execution state:
    /// operand stack, call frames, exception handlers, coroutine state.
    pub fiber: Fiber,
    /// Handle to the current fiber's FiberHandle, if it came from a
    /// `fiber/new` allocation. `None` for the root fiber (which lives
    /// directly on the VM, not behind a handle). Used to wire up
    /// `child.parent` back-pointers during fiber resume.
    pub current_fiber_handle: Option<FiberHandle>,
    /// Cached NaN-boxed Value for the current fiber. `None` for the root
    /// fiber. Used to set `child.parent_value` during resume chain wiring,
    /// so `fiber/parent` can return the original Value without re-allocating.
    pub current_fiber_value: Option<Value>,
    pub(crate) ffi: FFISubsystem,
    /// Modules currently being loaded (circular-import guard).
    /// Added before execution, removed after. If a module is in this set
    /// when import-file is called, it's a circular dependency.
    pub loading_modules: std::collections::HashSet<String>,
    pub closure_call_counts: FxHashMap<*const u8, usize>,
    pub location_map: LocationMap,
    pub tail_call_env_cache: Vec<Value>,
    pub env_cache: Vec<Value>,
    pub(crate) pending_tail_call: Option<TailCallInfo>,
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
    /// `(jit/rejections)` primitive and `ELLE_JIT_STATS` env var.
    pub jit_rejections: FxHashMap<*const u8, JitRejectionInfo>,
    /// Cached Expander for runtime `eval`. Avoids re-loading the prelude
    /// on every eval call. Taken out during eval, put back after.
    pub eval_expander: Option<crate::syntax::Expander>,
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
            lbox_params_mask: 0,
            lbox_locals_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            jit_code: None,
            lir_function: None,
            doc: None,
            syntax: None,
            vararg_kind: crate::hir::VarargKind::List,
            name: None,
        }),
        env: Rc::new(vec![]),
    })
}

impl VM {
    pub fn new() -> Self {
        // Install the root fiber heap before any allocation can happen.
        crate::value::fiber_heap::install_root_heap();

        let mut fiber = Fiber::new(root_closure(), SIG_OK);
        // Root fiber starts alive (it's the currently executing context)
        fiber.status = crate::value::FiberStatus::Alive;

        VM {
            fiber,
            current_fiber_handle: None, // root fiber has no handle
            current_fiber_value: None,  // root fiber has no Value
            ffi: FFISubsystem::new(),
            loading_modules: std::collections::HashSet::new(),
            closure_call_counts: FxHashMap::default(),
            location_map: LocationMap::new(),
            tail_call_env_cache: Vec::with_capacity(256),
            env_cache: Vec::with_capacity(256),
            pending_tail_call: None,
            error_loc: None,
            jit_cache: FxHashMap::default(),
            jit_rejections: FxHashMap::default(),
            docs: HashMap::new(),
            eval_expander: None,
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

    /// Record a closure call and return whether it's "hot" (called 10+ times)
    pub fn record_closure_call(&mut self, bytecode_ptr: *const u8) -> bool {
        let count = self.closure_call_counts.entry(bytecode_ptr).or_insert(0);
        *count += 1;
        *count >= 10
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
                    // Deliver `current_value` to the suspended sub-fiber.
                    // Inject it as the sub-fiber's signal value so that
                    // `do_fiber_resume` picks it up as the resume_value for
                    // `do_fiber_subsequent_resume`, which pushes it onto the
                    // I/O frame's stack (the return value of the I/O call).
                    handle.with_mut(|f| {
                        f.signal = Some((SIG_OK, current_value));
                    });

                    let handle = handle.clone();
                    let fiber_value = *fiber_value;
                    let (result_bits, result_value) = self.do_fiber_resume(&handle, fiber_value);
                    let mask = handle.with(|f| f.mask);

                    if result_bits.is_ok()
                        || (mask.covers(result_bits)
                            && !result_bits.contains(crate::value::SIG_TERMINAL))
                    {
                        // Sub-fiber completed or its signal was caught by mask.
                        // Clear child chain; result flows to next frame.
                        self.fiber.child = None;
                        self.fiber.child_value = None;
                        current_value = result_value;
                    } else {
                        // Sub-fiber suspended again (e.g. another I/O yield).
                        // Propagate: save the sub-fiber as a FiberResume frame
                        // again, then append remaining outer frames.
                        use crate::value::fiber::FiberStatus;
                        if result_bits.contains(crate::value::SIG_HALT) {
                            handle.with_mut(|f| f.status = FiberStatus::Dead);
                        }
                        if result_bits.contains(crate::value::SIG_ERROR) {
                            handle.with_mut(|f| f.status = FiberStatus::Error);
                        }
                        self.fiber.signal = Some((result_bits, result_value));

                        // Re-save the FiberResume frame for the next resume.
                        // The sub-fiber's suspension context is stored in f.suspended.
                        if !result_bits.contains(SIG_HALT) {
                            let resume_frame = SuspendedFrame::FiberResume {
                                handle: handle.clone(),
                                fiber_value,
                            };
                            let mut new_frames = vec![resume_frame];
                            // Append remaining outer frames after this one
                            for f in frames[i + 1..].iter() {
                                new_frames.push(f.clone());
                            }
                            self.fiber.suspended = Some(new_frames);
                        }

                        self.fiber.stack = saved_stack;
                        return result_bits;
                    }
                }

                SuspendedFrame::Bytecode(frame) => {
                    // Restore this frame's stack (empty for signal suspension)
                    self.fiber.stack.clear();
                    self.fiber.stack.extend(frame.stack.iter().copied());

                    // Push the value from the inner frame (or resume value for innermost)
                    self.fiber.stack.push(current_value);

                    let exec = self.execute_bytecode_from_ip(
                        &frame.bytecode,
                        &frame.constants,
                        &frame.env,
                        frame.ip,
                        &frame.location_map,
                    );

                    if exec.bits.is_ok() {
                        let (_, v) = self.fiber.signal.take().unwrap();
                        current_value = v;
                    } else {
                        // Non-OK signal (yield, error, user-defined).
                        // Save context for potential future resume if not already
                        // set (yield instruction sets it; fiber/signal does not).
                        // SIG_HALT is non-resumable — no suspended frame needed.
                        //
                        // Use the active bytecode/constants/env from ExecResult,
                        // not the original frame — a tail call may have switched
                        // to a different function's bytecode before the signal.
                        if !exec.bits.contains(SIG_HALT) && self.fiber.suspended.is_none() {
                            self.fiber.suspended =
                                Some(vec![SuspendedFrame::Bytecode(BytecodeFrame {
                                    bytecode: exec.bytecode,
                                    constants: exec.constants,
                                    env: exec.env,
                                    ip: exec.ip,
                                    stack: vec![],
                                    location_map: exec.location_map,
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

        assert_eq!(SIG_OK.bits(), 0);
        assert_eq!(SIG_ERROR.bits(), 1);
        assert_eq!(SIG_YIELD.bits(), 2);

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
