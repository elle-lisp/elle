use crate::error::{LocationMap, StackFrame};
use crate::ffi::FFISubsystem;
use crate::primitives::def::PrimitiveDoc;
use crate::value::fiber::CallFrame;
use crate::value::{
    Closure, Fiber, FiberHandle, SignalBits, SuspendedFrame, Value, SIG_HALT, SIG_OK, SIG_YIELD,
};
use crate::vm::scope::ScopeStack;
use rustc_hash::FxHashMap;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use crate::jit::JitCode;

// Re-export CallFrame from fiber (it now lives there)
pub use crate::value::fiber::CallFrame as FiberCallFrame;

type TailCallInfo = (Rc<Vec<u8>>, Rc<Vec<Value>>, Rc<Vec<Value>>);

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
    /// Global variable bindings (shared across all fibers)
    pub globals: Vec<Value>,
    pub ffi: FFISubsystem,
    pub modules: HashMap<String, HashMap<u32, Value>>,
    pub current_module: Option<String>,
    pub loaded_modules: HashSet<String>,
    pub module_search_paths: Vec<PathBuf>,
    pub scope_stack: ScopeStack,
    pub closure_call_counts: FxHashMap<*const u8, usize>,
    pub location_map: LocationMap,
    pub tail_call_env_cache: Vec<Value>,
    pub env_cache: Vec<Value>,
    pub pending_tail_call: Option<TailCallInfo>,
    pub current_source_loc: Option<crate::reader::SourceLoc>,
    /// JIT code cache: bytecode pointer → compiled native code.
    pub jit_cache: FxHashMap<*const u8, Rc<JitCode>>,
    /// Primitive documentation, keyed by name string for direct lookup.
    /// Populated during `register_primitives`.
    pub primitive_docs: HashMap<String, PrimitiveDoc>,
}

/// Create a dummy root closure for the root fiber.
/// The root fiber doesn't execute a closure directly — it's the
/// execution context for top-level bytecode. This closure is never
/// called; it exists only to satisfy Fiber's constructor.
fn root_closure() -> Rc<Closure> {
    use crate::effects::Effect;
    use crate::value::types::Arity;
    Rc::new(Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(0),
        env: Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![]),
        effect: Effect::none(),
        cell_params_mask: 0,
        symbol_names: Rc::new(HashMap::new()),
        location_map: Rc::new(LocationMap::new()),
        jit_code: None,
        lir_function: None,
    })
}

impl VM {
    pub fn new() -> Self {
        let mut fiber = Fiber::new(root_closure(), 0);
        // Root fiber starts alive (it's the currently executing context)
        fiber.status = crate::value::FiberStatus::Alive;

        VM {
            fiber,
            current_fiber_handle: None, // root fiber has no handle
            current_fiber_value: None,  // root fiber has no Value
            globals: vec![Value::UNDEFINED; 256],
            ffi: FFISubsystem::new(),
            modules: HashMap::new(),
            current_module: None,
            loaded_modules: HashSet::new(),
            module_search_paths: vec![PathBuf::from(".")],
            scope_stack: ScopeStack::new(),
            closure_call_counts: FxHashMap::default(),
            location_map: LocationMap::new(),
            tail_call_env_cache: Vec::with_capacity(256),
            env_cache: Vec::with_capacity(256),
            pending_tail_call: None,
            current_source_loc: None,
            jit_cache: FxHashMap::default(),
            primitive_docs: HashMap::new(),
        }
    }

    pub fn set_global(&mut self, sym_id: u32, value: Value) {
        let idx = sym_id as usize;
        if idx >= self.globals.len() {
            self.globals.resize(idx + 1, Value::UNDEFINED);
        }
        self.globals[idx] = value;
    }

    pub fn get_global(&self, sym_id: u32) -> Option<&Value> {
        let idx = sym_id as usize;
        self.globals.get(idx).filter(|v| !v.is_undefined())
    }

    /// Set the location map for mapping bytecode instructions to source locations
    pub fn set_location_map(&mut self, map: LocationMap) {
        self.location_map = map;
    }

    /// Get the location map for bytecode instruction lookups
    pub fn get_location_map(&self) -> &LocationMap {
        &self.location_map
    }

    /// Set the current source location for error reporting
    pub fn set_current_source_loc(&mut self, loc: Option<crate::reader::SourceLoc>) {
        self.current_source_loc = loc;
    }

    /// Get the current source location
    pub fn get_current_source_loc(&self) -> Option<&crate::reader::SourceLoc> {
        self.current_source_loc.as_ref()
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

    /// Define a module with exported symbols
    pub fn define_module(&mut self, name: String, exports: HashMap<u32, Value>) {
        self.modules.insert(name, exports);
    }

    /// Get a symbol from a module
    pub fn get_module_symbol(&self, module: &str, sym_id: u32) -> Option<&Value> {
        self.modules.get(module).and_then(|m| m.get(&sym_id))
    }

    /// Import a module (make it available)
    pub fn import_module(&mut self, name: String) {
        if self.modules.contains_key(&name) {
            // Module is now available for module:symbol references
        }
    }

    /// Set current module context
    pub fn set_current_module(&mut self, module: Option<String>) {
        self.current_module = module;
    }

    /// Get current module context
    pub fn current_module(&self) -> Option<&str> {
        self.current_module.as_deref()
    }

    /// Add a module search path
    pub fn add_module_search_path(&mut self, path: PathBuf) {
        self.module_search_paths.push(path);
    }

    /// Check if module is already loaded
    pub fn is_module_loaded(&self, module_path: &str) -> bool {
        self.loaded_modules.contains(module_path)
    }

    /// Mark module as loaded
    pub fn mark_module_loaded(&mut self, module_path: String) {
        self.loaded_modules.insert(module_path);
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

    pub fn push_call_frame(&mut self, name: String, ip: usize) {
        let frame_base = self.fiber.stack.len();
        self.fiber.call_depth += 1;
        self.fiber.call_stack.push(CallFrame {
            name,
            ip,
            frame_base,
        });
    }

    pub fn push_call_frame_with_base(&mut self, name: String, ip: usize, frame_base: usize) {
        self.fiber.call_depth += 1;
        self.fiber.call_stack.push(CallFrame {
            name,
            ip,
            frame_base,
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

    #[allow(dead_code)]
    pub fn with_stack_trace(&self, msg: String) -> String {
        let trace = self.format_stack_trace();
        format!("{}\nStack trace:\n{}", msg, trace)
    }

    /// Capture current call stack as trace frames
    pub fn capture_stack_trace(&self) -> Vec<StackFrame> {
        self.fiber
            .call_stack
            .iter()
            .rev()
            .map(|frame| {
                let location = self.location_map.get(&frame.ip).cloned();
                StackFrame {
                    function_name: Some(frame.name.clone()),
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

    pub fn ffi(&self) -> &FFISubsystem {
        &self.ffi
    }

    pub fn ffi_mut(&mut self) -> &mut FFISubsystem {
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

            // Restore this frame's stack (empty for signal suspension)
            self.fiber.stack.clear();
            self.fiber.stack.extend(frame.stack.iter().copied());

            // Push the value from the inner frame (or resume value for innermost)
            self.fiber.stack.push(current_value);

            let (bits, ip) = self.execute_bytecode_from_ip(
                &frame.bytecode,
                &frame.constants,
                &frame.env,
                frame.ip,
            );

            match bits {
                SIG_OK => {
                    let (_, v) = self.fiber.signal.take().unwrap();
                    current_value = v;
                }
                _ => {
                    // Non-OK signal (yield, error, user-defined).
                    // Save context for potential future resume if not already
                    // set (yield instruction sets it; fiber/signal does not).
                    // SIG_HALT is non-resumable — no suspended frame needed.
                    if bits != SIG_HALT && self.fiber.suspended.is_none() {
                        self.fiber.suspended = Some(vec![SuspendedFrame {
                            bytecode: frame.bytecode.clone(),
                            constants: frame.constants.clone(),
                            env: frame.env.clone(),
                            ip,
                            stack: vec![],
                        }]);
                    }

                    // For yield signals, merge remaining outer frames
                    if bits == SIG_YIELD && i + 1 < frames.len() {
                        if let Some(ref mut new_frames) = self.fiber.suspended {
                            for f in frames[i + 1..].iter() {
                                new_frames.push(f.clone());
                            }
                        }
                    }

                    self.fiber.stack = saved_stack;
                    return bits;
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

        assert_eq!(SIG_OK, 0);
        assert_eq!(SIG_ERROR, 1);
        assert_eq!(SIG_YIELD, 2);

        let mask = SIG_ERROR | SIG_YIELD;
        assert_ne!(mask & SIG_ERROR, 0);
        assert_ne!(mask & SIG_YIELD, 0);
        assert_eq!(mask & SIG_OK, 0);
    }

    #[test]
    fn test_capture_stack_trace() {
        let mut vm = VM::new();

        vm.push_call_frame("function_a".to_string(), 10);
        vm.push_call_frame("function_b".to_string(), 20);
        vm.push_call_frame("function_c".to_string(), 30);

        let trace = vm.capture_stack_trace();

        assert_eq!(trace.len(), 3);
        assert_eq!(trace[0].function_name, Some("function_c".to_string()));
        assert_eq!(trace[1].function_name, Some("function_b".to_string()));
        assert_eq!(trace[2].function_name, Some("function_a".to_string()));
    }

    #[test]
    fn test_wrap_error_with_trace() {
        let mut vm = VM::new();

        vm.push_call_frame("outer".to_string(), 5);
        vm.push_call_frame("inner".to_string(), 15);

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
