use crate::error::{LocationMap, StackFrame};
use crate::ffi::FFISubsystem;
use crate::value::{Condition, Coroutine, Value};
use crate::vm::scope::ScopeStack;
use smallvec::SmallVec;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

// Re-export ExceptionHandler from value::continuation where it's defined
pub use crate::value::continuation::ExceptionHandler;

type StackVec = SmallVec<[Value; 256]>;
type TailCallInfo = (Vec<u8>, Vec<Value>, Rc<Vec<Value>>);

/// Result of VM execution - can be normal completion or a yield
#[derive(Debug, Clone)]
pub enum VmResult {
    /// Normal completion with a value
    Done(Value),
    /// Coroutine yielded with a value and its continuation
    Yielded {
        /// The value being yielded
        value: Value,
        /// The continuation capturing the full frame chain
        continuation: Value,
    },
}

#[derive(Debug, Clone)]
pub struct CallFrame {
    pub name: String,
    pub ip: usize,
    pub frame_base: usize, // Stack index where this frame's locals start
}

pub struct VM {
    pub stack: StackVec,
    pub globals: HashMap<u32, Value>,
    pub call_depth: usize,
    pub call_stack: Vec<CallFrame>,
    pub ffi: FFISubsystem,
    pub modules: HashMap<String, HashMap<u32, Value>>, // Module name → exported symbols
    pub current_module: Option<String>,
    pub loaded_modules: HashSet<String>, // Track loaded module paths to prevent circular deps
    pub module_search_paths: Vec<PathBuf>, // Directories to search for modules
    pub scope_stack: ScopeStack,         // Runtime scope stack for variable management
    pub exception_handlers: Vec<ExceptionHandler>, // Stack of active exception handlers
    pub current_exception: Option<Rc<Condition>>, // Current exception being handled
    pub handling_exception: bool,        // True if we're currently in exception handler code
    pub closure_call_counts: std::collections::HashMap<*const u8, usize>, // Track closure call frequencies for JIT
    pub location_map: LocationMap, // Bytecode instruction index → source location mapping
    pub tail_call_env_cache: Vec<Value>, // Reusable environment vector for tail calls to avoid repeated allocations
    pub pending_tail_call: Option<TailCallInfo>, // (bytecode, constants, env) for pending tail call
    pub coroutine_stack: Vec<Rc<RefCell<Coroutine>>>, // Stack of active coroutines
    pub current_source_loc: Option<crate::reader::SourceLoc>, // Current top-level form's location
}

impl VM {
    pub fn new() -> Self {
        VM {
            stack: SmallVec::new(),
            globals: HashMap::new(),
            call_depth: 0,
            call_stack: Vec::new(),
            ffi: FFISubsystem::new(),
            modules: HashMap::new(),
            current_module: None,
            loaded_modules: HashSet::new(),
            module_search_paths: vec![PathBuf::from(".")],
            scope_stack: ScopeStack::new(),
            exception_handlers: Vec::new(),
            current_exception: None,
            handling_exception: false,
            closure_call_counts: std::collections::HashMap::new(),
            location_map: LocationMap::new(),
            tail_call_env_cache: Vec::with_capacity(256),
            pending_tail_call: None,
            coroutine_stack: Vec::new(),
            current_source_loc: None,
        }
    }

    pub fn set_global(&mut self, sym_id: u32, value: Value) {
        self.globals.insert(sym_id, value);
    }

    pub fn get_global(&self, sym_id: u32) -> Option<&Value> {
        self.globals.get(&sym_id)
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
        self.current_source_loc = loc.clone();
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
        self.call_stack.last().map(|f| f.frame_base).unwrap_or(0)
    }

    pub fn push_call_frame(&mut self, name: String, ip: usize) {
        let frame_base = self.stack.len(); // Current stack top becomes frame base
        self.call_depth += 1;
        self.call_stack.push(CallFrame {
            name,
            ip,
            frame_base,
        });
    }

    pub fn push_call_frame_with_base(&mut self, name: String, ip: usize, frame_base: usize) {
        self.call_depth += 1;
        self.call_stack.push(CallFrame {
            name,
            ip,
            frame_base,
        });
    }

    pub fn pop_call_frame(&mut self) {
        if self.call_depth > 0 {
            self.call_depth -= 1;
            self.call_stack.pop();
        }
    }

    pub fn format_stack_trace(&self) -> String {
        if self.call_stack.is_empty() {
            "No call frames".to_string()
        } else {
            let mut trace = String::new();
            for (i, frame) in self.call_stack.iter().rev().enumerate() {
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
        self.call_stack
            .iter()
            .rev() // Most recent first
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

    /// Enter a coroutine context (push onto coroutine stack)
    pub fn enter_coroutine(&mut self, co: Rc<RefCell<Coroutine>>) {
        self.coroutine_stack.push(co);
    }

    /// Exit a coroutine context (pop from coroutine stack)
    pub fn exit_coroutine(&mut self) -> Option<Rc<RefCell<Coroutine>>> {
        self.coroutine_stack.pop()
    }

    /// Get the currently executing coroutine, if any
    pub fn current_coroutine(&self) -> Option<&Rc<RefCell<Coroutine>>> {
        self.coroutine_stack.last()
    }

    /// Check if we're currently inside a coroutine
    pub fn in_coroutine(&self) -> bool {
        !self.coroutine_stack.is_empty()
    }

    /// Resume execution from a saved continuation.
    ///
    /// A continuation captures the full chain of frames from a yield point
    /// up to the coroutine boundary. This method replays the chain from
    /// innermost to outermost, threading the resume value through.
    ///
    /// Frame ordering: `frames\[0\]` = innermost (yielder), `frames\[last\]` = outermost (caller)
    ///
    /// # Arguments
    /// * `continuation` - A Value containing ContinuationData
    /// * `resume_value` - The value to resume with (becomes the yield expression's result)
    ///
    /// # Returns
    /// * `VmResult::Done(value)` - All frames completed, returning final value
    /// * `VmResult::Yielded { value, continuation }` - Re-yielded with new continuation
    pub fn resume_continuation(
        &mut self,
        continuation: Value,
        resume_value: Value,
    ) -> Result<VmResult, String> {
        let cont_data = continuation
            .as_continuation()
            .ok_or("Expected continuation value")?;

        let frames = &cont_data.frames;
        if frames.is_empty() {
            return Ok(VmResult::Done(resume_value));
        }

        // Save current stack state
        let saved_stack = std::mem::take(&mut self.stack);

        // Execute from innermost frame (index 0) outward to outermost (last index)
        let mut current_value = resume_value;

        for i in 0..frames.len() {
            let frame = &frames[i];

            // Restore this frame's stack
            self.stack.clear();
            self.stack.extend(frame.stack.iter().copied());

            // Push the value from the inner frame (or resume value for innermost)
            self.stack.push(current_value);

            // Execute with the frame's saved exception handler state
            let result = self.execute_bytecode_from_ip_with_state(
                &frame.bytecode,
                &frame.constants,
                Some(&frame.env),
                frame.ip,
                frame.exception_handlers.clone(),
                frame.handling_exception,
            )?;

            match result {
                VmResult::Done(v) => {
                    // Check if an exception occurred in this frame
                    // If so, and there are more outer frames, let them handle it
                    if self.current_exception.is_some() && i + 1 < frames.len() {
                        // The exception will be handled by the next outer frame's
                        // exception handlers (which we'll restore when we execute it)
                        // Pass NIL as the "return value" since we're propagating an exception
                        current_value = Value::NIL;
                    } else {
                        current_value = v;
                    }
                    // Continue with next outer frame
                }
                VmResult::Yielded {
                    value,
                    continuation: new_cont,
                } => {
                    // Re-yielded during resume! Need to append remaining outer frames
                    // to the new continuation
                    if i + 1 < frames.len() {
                        let mut new_cont_data = new_cont
                            .as_continuation()
                            .ok_or("Expected continuation")?
                            .as_ref()
                            .clone();
                        // Append frames[i+1..] (the remaining outer frames)
                        for f in frames[i + 1..].iter() {
                            new_cont_data.frames.push(f.clone());
                        }
                        let merged_cont = Value::continuation(new_cont_data);
                        self.stack = saved_stack;
                        return Ok(VmResult::Yielded {
                            value,
                            continuation: merged_cont,
                        });
                    }
                    self.stack = saved_stack;
                    return Ok(VmResult::Yielded {
                        value,
                        continuation: new_cont,
                    });
                }
            }
        }

        self.stack = saved_stack;
        Ok(VmResult::Done(current_value))
    }
}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod coroutine_vm_tests {
    use super::*;
    use crate::effects::Effect;
    use crate::value::{Arity, Closure};

    #[test]
    fn test_vm_coroutine_stack_operations() {
        let mut vm = VM::new();

        // Initially not in coroutine
        assert!(!vm.in_coroutine());
        assert!(vm.current_coroutine().is_none());

        // Create a test coroutine
        let closure = Rc::new(Closure {
            bytecode: Rc::new(vec![]),
            arity: Arity::Exact(0),
            env: Rc::new(vec![]),
            num_locals: 0,
            num_captures: 0,
            constants: Rc::new(vec![]),
            source_ast: None,
            effect: Effect::Pure,
            cell_params_mask: 0,
            symbol_names: Rc::new(std::collections::HashMap::new()),
        });
        let co = Rc::new(RefCell::new(Coroutine::new(closure)));

        // Enter coroutine
        vm.enter_coroutine(co.clone());
        assert!(vm.in_coroutine());
        assert!(vm.current_coroutine().is_some());

        // Exit coroutine
        let exited = vm.exit_coroutine();
        assert!(exited.is_some());
        assert!(!vm.in_coroutine());
    }

    #[test]
    fn test_vm_nested_coroutines() {
        let mut vm = VM::new();

        let make_co = || {
            let closure = Rc::new(Closure {
                bytecode: Rc::new(vec![]),
                arity: Arity::Exact(0),
                env: Rc::new(vec![]),
                num_locals: 0,
                num_captures: 0,
                constants: Rc::new(vec![]),
                source_ast: None,
                effect: Effect::Pure,
                cell_params_mask: 0,
                symbol_names: Rc::new(std::collections::HashMap::new()),
            });
            Rc::new(RefCell::new(Coroutine::new(closure)))
        };

        let co1 = make_co();
        let co2 = make_co();

        vm.enter_coroutine(co1.clone());
        vm.enter_coroutine(co2.clone());

        // Should be at co2
        assert!(Rc::ptr_eq(vm.current_coroutine().unwrap(), &co2));

        vm.exit_coroutine();

        // Should be at co1
        assert!(Rc::ptr_eq(vm.current_coroutine().unwrap(), &co1));

        vm.exit_coroutine();
        assert!(!vm.in_coroutine());
    }

    #[test]
    fn test_vm_result_enum() {
        use crate::value::ContinuationData;

        let done = VmResult::Done(Value::int(42));
        // Create a dummy continuation for testing
        let cont_data = ContinuationData { frames: vec![] };
        let yielded = VmResult::Yielded {
            value: Value::int(100),
            continuation: Value::continuation(cont_data),
        };

        if let VmResult::Done(v) = done {
            if let Some(n) = v.as_int() {
                assert_eq!(n, 42);
            } else {
                panic!("Expected Done");
            }
        } else {
            panic!("Expected Done");
        }

        if let VmResult::Yielded { value, .. } = yielded {
            if let Some(n) = value.as_int() {
                assert_eq!(n, 100);
            } else {
                panic!("Expected Yielded");
            }
        } else {
            panic!("Expected Yielded");
        }
    }

    #[test]
    fn test_capture_stack_trace() {
        let mut vm = VM::new();

        // Push some call frames
        vm.push_call_frame("function_a".to_string(), 10);
        vm.push_call_frame("function_b".to_string(), 20);
        vm.push_call_frame("function_c".to_string(), 30);

        // Capture the stack trace
        let trace = vm.capture_stack_trace();

        // Should have 3 frames in reverse order (most recent first)
        assert_eq!(trace.len(), 3);
        assert_eq!(trace[0].function_name, Some("function_c".to_string()));
        assert_eq!(trace[1].function_name, Some("function_b".to_string()));
        assert_eq!(trace[2].function_name, Some("function_a".to_string()));
    }

    #[test]
    fn test_wrap_error_with_trace() {
        let mut vm = VM::new();

        // Push some call frames
        vm.push_call_frame("outer".to_string(), 5);
        vm.push_call_frame("inner".to_string(), 15);

        // Wrap an error
        let error_msg = "Something went wrong".to_string();
        let wrapped = vm.wrap_error(error_msg);

        // Should contain the original error message
        assert!(wrapped.contains("Something went wrong"));
        // Should contain the function names
        assert!(wrapped.contains("inner"));
        assert!(wrapped.contains("outer"));
    }

    #[test]
    fn test_wrap_error_empty_stack() {
        let vm = VM::new();

        // Wrap an error with empty call stack
        let error_msg = "Error with no context".to_string();
        let wrapped = vm.wrap_error(error_msg.clone());

        // Should just return the original error
        assert_eq!(wrapped, error_msg);
    }
}
