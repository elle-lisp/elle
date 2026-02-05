use crate::ffi::FFISubsystem;
use crate::value::Value;
use crate::vm::scope::ScopeStack;
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

type StackVec = SmallVec<[Value; 256]>;

#[derive(Debug, Clone)]
pub struct CallFrame {
    pub name: String,
    pub ip: usize,
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
        }
    }

    pub fn set_global(&mut self, sym_id: u32, value: Value) {
        self.globals.insert(sym_id, value);
    }

    pub fn get_global(&self, sym_id: u32) -> Option<&Value> {
        self.globals.get(&sym_id)
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
}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}
