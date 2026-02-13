use crate::error::LocationMap;
use crate::ffi::FFISubsystem;
use crate::value::{Condition, Coroutine, CoroutineContext, Value};
use crate::vm::scope::ScopeStack;
use smallvec::SmallVec;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

type StackVec = SmallVec<[Value; 256]>;
type TailCallInfo = (Vec<u8>, Vec<Value>, Rc<Vec<Value>>);

/// Result of VM execution - can be normal completion or a yield
#[derive(Debug, Clone)]
pub enum VmResult {
    /// Normal completion with a value
    Done(Value),
    /// Coroutine yielded with a value
    Yielded(Value),
}

#[derive(Debug, Clone)]
pub struct CallFrame {
    pub name: String,
    pub ip: usize,
}

#[derive(Debug, Clone)]
pub struct ExceptionHandler {
    pub handler_offset: i16,
    pub finally_offset: Option<i16>,
    pub stack_depth: usize,
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

/// Exception type hierarchy (baked into VM for inheritance checking)
/// ID 1: condition (base)
///   ID 2: error
///     ID 3: type-error
///     ID 4: division-by-zero
///     ID 5: undefined-variable
///     ID 6: arity-error
///   ID 7: warning
///     ID 8: style-warning
pub fn exception_parent(exception_id: u32) -> Option<u32> {
    match exception_id {
        2 => Some(1), // error -> condition
        3 => Some(2), // type-error -> error
        4 => Some(2), // division-by-zero -> error
        5 => Some(2), // undefined-variable -> error
        6 => Some(2), // arity-error -> error
        7 => Some(1), // warning -> condition
        8 => Some(7), // style-warning -> warning
        _ => None,
    }
}

/// Check if child exception ID is a subclass of parent exception ID
pub fn is_exception_subclass(child_id: u32, parent_id: u32) -> bool {
    if child_id == parent_id {
        return true;
    }

    let mut current = child_id;
    while let Some(parent) = exception_parent(current) {
        if parent == parent_id {
            return true;
        }
        current = parent;
    }
    false
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

    pub fn push_call_frame(&mut self, name: String, ip: usize) {
        self.call_depth += 1;
        self.call_stack.push(CallFrame { name, ip });
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

    /// Resume execution from a saved coroutine context
    ///
    /// This restores the saved state and continues execution from where
    /// the coroutine yielded. The resume_value becomes the result of the
    /// yield expression.
    pub fn resume_from_context(
        &mut self,
        context: CoroutineContext,
        resume_value: Value,
        bytecode: &[u8],
        constants: &[Value],
    ) -> Result<VmResult, String> {
        // Save current state
        let saved_stack = std::mem::take(&mut self.stack);

        // Restore coroutine's state
        self.stack = context.stack.into();

        // Push the resume value (this is what the yield expression evaluates to)
        self.stack.push(resume_value);

        // Get the closure env from the current coroutine
        let (closure_env, num_locals, num_captures) = {
            let co = self
                .current_coroutine()
                .ok_or("resume_from_context called outside coroutine")?;
            let co_ref = co.borrow();
            (
                co_ref.closure.env.clone(),
                co_ref.closure.num_locals,
                co_ref.closure.num_captures,
            )
        };
        // Set up the environment for the coroutine
        // The closure environment contains: [captures..., parameters..., locals...]
        // We need to allocate space for locals if they haven't been allocated yet
        let mut env = (*closure_env).clone();

        // Calculate number of locally-defined variables
        // num_locals = params.len() + captures.len() + locals.len()
        // Since a coroutine has no parameters, we need to allocate space for all locals
        let num_locally_defined = num_locals.saturating_sub(num_captures);

        // Add empty LocalCells for locally-defined variables if not already present
        for _ in env.len()..num_captures + num_locally_defined {
            let empty_cell = Value::LocalCell(std::rc::Rc::new(std::cell::RefCell::new(Box::new(
                Value::Nil,
            ))));
            env.push(empty_cell);
        }

        let env_rc = std::rc::Rc::new(env);

        // Execute from saved IP with the closure's environment
        let result = self.execute_bytecode_from_ip(bytecode, constants, Some(&env_rc), context.ip);

        // Restore our state (in case we need to continue after coroutine completes)
        self.stack = saved_stack;

        result
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
    use crate::compiler::effects::Effect;
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
        let done = VmResult::Done(Value::Int(42));
        let yielded = VmResult::Yielded(Value::Int(100));

        match done {
            VmResult::Done(Value::Int(n)) => assert_eq!(n, 42),
            _ => panic!("Expected Done"),
        }

        match yielded {
            VmResult::Yielded(Value::Int(n)) => assert_eq!(n, 100),
            _ => panic!("Expected Yielded"),
        }
    }
}
