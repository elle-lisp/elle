use super::runtime_scope::RuntimeScope;
use crate::compiler::scope::ScopeType;
use crate::value::Value;

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
                return Some(*value);
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let value = Value::int(123);

        stack.define_local(sym_id, value);
        assert_eq!(stack.get(sym_id), Some(value));
    }

    #[test]
    fn test_variable_lookup_walks_up() {
        let mut stack = ScopeStack::new();
        let x_id = 1;
        let y_id = 2;
        let x_val = Value::int(10);
        let y_val = Value::int(20);

        // Define x in global
        stack.define_local(x_id, x_val);

        // Push a function scope
        stack.push(ScopeType::Function);

        // Define y locally
        stack.define_local(y_id, y_val);

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
        let x_val1 = Value::int(10);
        let x_val2 = Value::int(20);

        // Define in global
        stack.define_local(x_id, x_val1);

        // Push function scope (without defining x)
        stack.push(ScopeType::Function);

        // Set should find x in parent and update it
        assert!(stack.set(x_id, x_val2));
        assert_eq!(stack.get(x_id), Some(x_val2));

        // Go back to global - should see the update
        stack.pop();
        assert_eq!(stack.get(x_id), Some(x_val2));
    }

    #[test]
    fn test_set_at_depth() {
        let mut stack = ScopeStack::new();
        let sym_id = 42;
        let val1 = Value::int(1);
        let val2 = Value::int(2);

        // Define in global (depth 0 from global)
        stack.define_local(sym_id, val1);

        // Push scope
        stack.push(ScopeType::Block);

        // Set at depth 1 (parent scope/global)
        assert!(stack.set_at_depth(1, sym_id, val2));

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

        let a_val = Value::int(10);
        let b_val = Value::int(20);
        let c_val = Value::int(30);

        // Global scope: define a
        stack.define_local(a_id, a_val);

        // Function scope: define b
        stack.push(ScopeType::Function);
        stack.define_local(b_id, b_val);

        // Block scope: define c
        stack.push(ScopeType::Block);
        stack.define_local(c_id, c_val);

        // Can see all three
        assert_eq!(stack.get(a_id), Some(a_val));
        assert_eq!(stack.get(b_id), Some(b_val));
        assert_eq!(stack.get(c_id), Some(c_val));

        // Pop block
        assert!(stack.pop());
        assert_eq!(stack.get(a_id), Some(a_val));
        assert_eq!(stack.get(b_id), Some(b_val));
        assert_eq!(stack.get(c_id), None); // c no longer visible

        // Pop function
        assert!(stack.pop());
        assert_eq!(stack.get(a_id), Some(a_val));
        assert_eq!(stack.get(b_id), None); // b no longer visible
    }

    #[test]
    fn test_total_variables() {
        let mut stack = ScopeStack::new();

        stack.define_local(1, Value::int(1));
        assert_eq!(stack.total_variables(), 1);

        stack.push(ScopeType::Function);
        stack.define_local(2, Value::int(2));
        stack.define_local(3, Value::int(3));
        assert_eq!(stack.total_variables(), 3);

        stack.push(ScopeType::Block);
        stack.define_local(4, Value::int(4));
        assert_eq!(stack.total_variables(), 4);

        stack.pop();
        assert_eq!(stack.total_variables(), 3);
    }
}
