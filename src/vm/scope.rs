use crate::compiler::scope::ScopeType;
use crate::value::Value;
use std::collections::HashMap;

// These handlers will be defined below
// For now, just declare them as they'll be implemented step by step

/// Represents a single scope level at runtime
#[derive(Debug, Clone)]
pub struct RuntimeScope {
    /// Variables defined at this scope level (symbol_id -> Value)
    pub variables: HashMap<u32, Value>,
    /// Type of this scope
    pub scope_type: ScopeType,
}

impl RuntimeScope {
    /// Create a new runtime scope
    pub fn new(scope_type: ScopeType) -> Self {
        RuntimeScope {
            variables: HashMap::new(),
            scope_type,
        }
    }

    /// Define a variable in this scope
    pub fn define(&mut self, sym_id: u32, value: Value) {
        self.variables.insert(sym_id, value);
    }

    /// Get a variable from this scope
    pub fn get(&self, sym_id: u32) -> Option<&Value> {
        self.variables.get(&sym_id)
    }

    /// Get a mutable reference to a variable
    pub fn get_mut(&mut self, sym_id: u32) -> Option<&mut Value> {
        self.variables.get_mut(&sym_id)
    }

    /// Set a variable in this scope (returns old value if present)
    pub fn set(&mut self, sym_id: u32, value: Value) -> Option<Value> {
        self.variables.insert(sym_id, value)
    }

    /// Check if a variable is defined in this scope
    pub fn contains(&self, sym_id: u32) -> bool {
        self.variables.contains_key(&sym_id)
    }
}

/// Manages the runtime scope stack during execution
pub struct ScopeStack {
    /// Stack of scopes (index 0 is global, higher indices are nested)
    stack: Vec<RuntimeScope>,
}

impl ScopeStack {
    /// Create a new scope stack with global scope
    pub fn new() -> Self {
        let stack = vec![RuntimeScope::new(ScopeType::Global)];
        ScopeStack { stack }
    }

    /// Push a new scope onto the stack
    pub fn push(&mut self, scope_type: ScopeType) {
        self.stack.push(RuntimeScope::new(scope_type));
    }

    /// Pop the current scope from the stack (keep global)
    pub fn pop(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.stack.pop();
            true
        } else {
            false
        }
    }

    /// Get the current depth (number of scopes, 0 = global)
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Get the scope type at a given depth (0 = current, 1 = parent, etc.)
    pub fn scope_type_at_depth(&self, depth: usize) -> Option<ScopeType> {
        if depth >= self.stack.len() {
            return None;
        }
        let index = self.stack.len() - 1 - depth;
        Some(self.stack[index].scope_type)
    }

    /// Define a variable in the current scope
    pub fn define_local(&mut self, sym_id: u32, value: Value) {
        if let Some(scope) = self.stack.last_mut() {
            scope.define(sym_id, value);
        }
    }

    /// Get a variable, starting from current scope and walking up
    pub fn get(&self, sym_id: u32) -> Option<Value> {
        // Search from current scope back to global
        for scope in self.stack.iter().rev() {
            if let Some(value) = scope.get(sym_id) {
                return Some(value.clone());
            }
        }
        None
    }

    /// Get a variable at a specific depth
    /// depth = 0 means current scope
    /// depth = 1 means parent scope
    /// etc.
    pub fn get_at_depth(&self, depth: usize, sym_id: u32) -> Option<Value> {
        if depth >= self.stack.len() {
            return None;
        }
        let index = self.stack.len() - 1 - depth;
        self.stack[index].get(sym_id).cloned()
    }

    /// Set a variable, starting from current scope and walking up
    /// Returns true if variable was found and set, false otherwise
    pub fn set(&mut self, sym_id: u32, value: Value) -> bool {
        // Search from current scope back to global
        for scope in self.stack.iter_mut().rev() {
            if scope.contains(sym_id) {
                scope.set(sym_id, value);
                return true;
            }
        }
        false
    }

    /// Set a variable at a specific depth
    /// Returns true if variable was found and set
    pub fn set_at_depth(&mut self, depth: usize, sym_id: u32, value: Value) -> bool {
        if depth >= self.stack.len() {
            return false;
        }
        let index = self.stack.len() - 1 - depth;
        if self.stack[index].contains(sym_id) {
            self.stack[index].set(sym_id, value);
            true
        } else {
            false
        }
    }

    /// Check if a variable is defined locally (in current scope only)
    pub fn is_defined_local(&self, sym_id: u32) -> bool {
        if let Some(scope) = self.stack.last() {
            scope.contains(sym_id)
        } else {
            false
        }
    }

    /// Get the total number of variables across all scopes
    pub fn total_variables(&self) -> usize {
        self.stack.iter().map(|s| s.variables.len()).sum()
    }

    /// Get a mutable reference to the current scope
    pub fn current_scope_mut(&mut self) -> Option<&mut RuntimeScope> {
        self.stack.last_mut()
    }

    /// Get an immutable reference to the current scope
    pub fn current_scope(&self) -> Option<&RuntimeScope> {
        self.stack.last()
    }

    /// Get a mutable reference to a scope at a specific depth
    pub fn scope_mut_at_depth(&mut self, depth: usize) -> Option<&mut RuntimeScope> {
        if depth >= self.stack.len() {
            return None;
        }
        let index = self.stack.len() - 1 - depth;
        Some(&mut self.stack[index])
    }
}

impl Default for ScopeStack {
    fn default() -> Self {
        Self::new()
    }
}

/// Instruction handlers for scope management
/// These are called by the VM to manage runtime scopes
use crate::vm::core::VM;

/// Handle PushScope instruction
pub fn handle_push_scope(vm: &mut VM, scope_type_byte: u8) -> Result<(), String> {
    // Convert byte to ScopeType
    let scope_type = match scope_type_byte {
        0 => ScopeType::Global,
        1 => ScopeType::Function,
        2 => ScopeType::Block,
        3 => ScopeType::Loop,
        4 => ScopeType::Let,
        _ => return Err(format!("Invalid scope type: {}", scope_type_byte)),
    };

    vm.scope_stack.push(scope_type);
    Ok(())
}

/// Handle PopScope instruction
pub fn handle_pop_scope(vm: &mut VM) -> Result<(), String> {
    if !vm.scope_stack.pop() {
        return Err("Cannot pop global scope".to_string());
    }
    Ok(())
}

/// Handle LoadScoped instruction
pub fn handle_load_scoped(_vm: &mut VM, bytecode: &[u8], ip: &mut usize) -> Result<(), String> {
    let depth = bytecode[*ip] as usize;
    *ip += 1;
    let index = bytecode[*ip] as usize;
    *ip += 1;

    // This instruction is for future use - currently variables use LoadUpvalue
    // For now, just treat as a no-op to avoid breaking existing code
    let _ = depth;
    let _ = index;
    Ok(())
}

/// Handle StoreScoped instruction
pub fn handle_store_scoped(vm: &mut VM, bytecode: &[u8], ip: &mut usize) -> Result<(), String> {
    let depth = bytecode[*ip] as usize;
    *ip += 1;
    let index = bytecode[*ip] as usize;
    *ip += 1;

    // Pop value from stack
    let value = vm.stack.pop().ok_or("Stack underflow")?;

    // Store to scope at the specified depth
    if !vm.scope_stack.set_at_depth(depth, index as u32, value) {
        return Err(format!(
            "Variable not found at depth {} index {}",
            depth, index
        ));
    }

    Ok(())
}

/// Handle DefineLocal instruction
pub fn handle_define_local(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    constants: &[Value],
) -> Result<(), String> {
    // Read symbol index from bytecode
    let high = bytecode[*ip] as u16;
    let low = bytecode[*ip + 1] as u16;
    *ip += 2;
    let sym_idx = (high << 8) | low;

    // Pop value from stack
    let value = vm.stack.pop().ok_or("Stack underflow")?;

    // Get the symbol ID from constants
    let sym_id = if let Value::Symbol(id) = constants[sym_idx as usize] {
        id.0
    } else {
        return Err("Expected symbol in constants".to_string());
    };

    // Define in current scope
    vm.scope_stack.define_local(sym_id, value);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_scope_creation() {
        let scope = RuntimeScope::new(ScopeType::Global);
        assert_eq!(scope.scope_type, ScopeType::Global);
        assert!(scope.variables.is_empty());
    }

    #[test]
    fn test_runtime_scope_define_and_get() {
        let mut scope = RuntimeScope::new(ScopeType::Block);
        let sym_id = 42;
        let value = Value::Int(123);

        scope.define(sym_id, value.clone());
        assert_eq!(scope.get(sym_id), Some(&value));
        assert!(scope.contains(sym_id));
    }

    #[test]
    fn test_scope_stack_creation() {
        let stack = ScopeStack::new();
        assert_eq!(stack.depth(), 1); // Just global
    }

    #[test]
    fn test_scope_stack_push_pop() {
        let mut stack = ScopeStack::new();
        assert_eq!(stack.depth(), 1);

        stack.push(ScopeType::Function);
        assert_eq!(stack.depth(), 2);

        assert!(stack.pop());
        assert_eq!(stack.depth(), 1);

        // Can't pop global
        assert!(!stack.pop());
        assert_eq!(stack.depth(), 1);
    }

    #[test]
    fn test_define_local() {
        let mut stack = ScopeStack::new();
        let sym_id = 42;
        let value = Value::Int(123);

        stack.define_local(sym_id, value.clone());
        assert_eq!(stack.get(sym_id), Some(value));
    }

    #[test]
    fn test_variable_lookup_walks_up() {
        let mut stack = ScopeStack::new();
        let x_id = 1;
        let y_id = 2;
        let x_val = Value::Int(10);
        let y_val = Value::Int(20);

        // Define x in global
        stack.define_local(x_id, x_val.clone());

        // Push a function scope
        stack.push(ScopeType::Function);

        // Define y locally
        stack.define_local(y_id, y_val.clone());

        // Should find both
        assert_eq!(stack.get(x_id), Some(x_val));
        assert_eq!(stack.get(y_id), Some(y_val));

        // y should be local
        assert!(stack.is_defined_local(y_id));
        assert!(!stack.is_defined_local(x_id));
    }

    #[test]
    fn test_set_walks_up_scopes() {
        let mut stack = ScopeStack::new();
        let x_id = 1;
        let x_val1 = Value::Int(10);
        let x_val2 = Value::Int(20);

        // Define in global
        stack.define_local(x_id, x_val1);

        // Push function scope (without defining x)
        stack.push(ScopeType::Function);

        // Set should find x in parent and update it
        assert!(stack.set(x_id, x_val2.clone()));
        assert_eq!(stack.get(x_id), Some(x_val2.clone()));

        // Go back to global - should see the update
        stack.pop();
        assert_eq!(stack.get(x_id), Some(x_val2));
    }

    #[test]
    fn test_set_at_depth() {
        let mut stack = ScopeStack::new();
        let sym_id = 42;
        let val1 = Value::Int(1);
        let val2 = Value::Int(2);

        // Define in global (depth 0 from global)
        stack.define_local(sym_id, val1);

        // Push scope
        stack.push(ScopeType::Block);

        // Set at depth 1 (parent scope/global)
        assert!(stack.set_at_depth(1, sym_id, val2.clone()));

        // Verify it changed in global
        stack.pop();
        assert_eq!(stack.get(sym_id), Some(val2));
    }

    #[test]
    fn test_scope_type_at_depth() {
        let mut stack = ScopeStack::new();
        assert_eq!(stack.scope_type_at_depth(0), Some(ScopeType::Global));

        stack.push(ScopeType::Function);
        assert_eq!(stack.scope_type_at_depth(0), Some(ScopeType::Function));
        assert_eq!(stack.scope_type_at_depth(1), Some(ScopeType::Global));

        stack.push(ScopeType::Loop);
        assert_eq!(stack.scope_type_at_depth(0), Some(ScopeType::Loop));
        assert_eq!(stack.scope_type_at_depth(1), Some(ScopeType::Function));
        assert_eq!(stack.scope_type_at_depth(2), Some(ScopeType::Global));
    }

    #[test]
    fn test_nested_scopes() {
        let mut stack = ScopeStack::new();

        let a_id = 1;
        let b_id = 2;
        let c_id = 3;

        let a_val = Value::Int(10);
        let b_val = Value::Int(20);
        let c_val = Value::Int(30);

        // Global scope: define a
        stack.define_local(a_id, a_val.clone());

        // Function scope: define b
        stack.push(ScopeType::Function);
        stack.define_local(b_id, b_val.clone());

        // Block scope: define c
        stack.push(ScopeType::Block);
        stack.define_local(c_id, c_val.clone());

        // Can see all three
        assert_eq!(stack.get(a_id), Some(a_val.clone()));
        assert_eq!(stack.get(b_id), Some(b_val.clone()));
        assert_eq!(stack.get(c_id), Some(c_val.clone()));

        // Pop block
        assert!(stack.pop());
        assert_eq!(stack.get(a_id), Some(a_val.clone()));
        assert_eq!(stack.get(b_id), Some(b_val.clone()));
        assert_eq!(stack.get(c_id), None); // c no longer visible

        // Pop function
        assert!(stack.pop());
        assert_eq!(stack.get(a_id), Some(a_val));
        assert_eq!(stack.get(b_id), None); // b no longer visible
    }

    #[test]
    fn test_total_variables() {
        let mut stack = ScopeStack::new();

        stack.define_local(1, Value::Int(1));
        assert_eq!(stack.total_variables(), 1);

        stack.push(ScopeType::Function);
        stack.define_local(2, Value::Int(2));
        stack.define_local(3, Value::Int(3));
        assert_eq!(stack.total_variables(), 3);

        stack.push(ScopeType::Block);
        stack.define_local(4, Value::Int(4));
        assert_eq!(stack.total_variables(), 4);

        stack.pop();
        assert_eq!(stack.total_variables(), 3);
    }
}
