use crate::value::SymbolId;
use std::collections::HashMap;

/// Type of scope (affects variable binding semantics)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeType {
    /// Global scope (top-level defines)
    Global,
    /// Function/lambda scope (parameters and captures)
    Function,
    /// Block scope (let, begin, etc)
    Block,
    /// Loop scope (while, for loop bodies)
    Loop,
    /// Let-binding scope
    Let,
}

/// Binding type (what kind of variable is it?)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingType {
    /// Function parameter
    Parameter,
    /// Local variable (defined in this scope)
    Local,
    /// Captured variable (from parent scope)
    Captured,
}

/// Information about a variable binding at compile time
#[derive(Debug, Clone)]
pub struct VariableBinding {
    /// The symbol ID of the variable
    pub symbol_id: SymbolId,
    /// Type of binding (parameter, local, or captured)
    pub binding_type: BindingType,
    /// Distance to the scope that defines this variable
    /// 0 = defined in current scope
    /// 1 = defined in parent scope
    /// 2 = defined in grandparent scope, etc.
    pub depth: usize,
    /// Index within the scope where this variable is stored
    /// Used to distinguish multiple variables at the same depth
    pub index: usize,
}

/// Represents a single scope level (function, block, loop, etc.)
#[derive(Debug, Clone)]
pub struct ScopeFrame {
    /// Variables defined at this scope level
    pub variables: HashMap<u32, VariableBinding>,
    /// Type of this scope
    pub scope_type: ScopeType,
    /// Depth relative to global (0 = global, 1 = first level, etc.)
    pub depth: usize,
}

impl ScopeFrame {
    /// Create a new scope frame
    pub fn new(scope_type: ScopeType, depth: usize) -> Self {
        ScopeFrame {
            variables: HashMap::new(),
            scope_type,
            depth,
        }
    }

    /// Add a variable binding to this scope
    pub fn add_variable(&mut self, sym_id: SymbolId, binding_type: BindingType, index: usize) {
        self.variables.insert(
            sym_id.0,
            VariableBinding {
                symbol_id: sym_id,
                binding_type,
                depth: 0,
                index,
            },
        );
    }

    /// Check if a variable is defined in this scope
    pub fn contains(&self, sym_id: SymbolId) -> bool {
        self.variables.contains_key(&sym_id.0)
    }

    /// Get the binding for a variable if it's in this scope
    pub fn get(&self, sym_id: SymbolId) -> Option<&VariableBinding> {
        self.variables.get(&sym_id.0)
    }
}

/// Manages the stack of scopes during compilation
pub struct CompileScope {
    /// Stack of scope frames
    frames: Vec<ScopeFrame>,
}

impl CompileScope {
    /// Create a new scope manager starting with global scope
    pub fn new() -> Self {
        let frames = vec![ScopeFrame::new(ScopeType::Global, 0)];
        CompileScope { frames }
    }

    /// Get the current depth (number of scopes deep, 0 = global)
    pub fn current_depth(&self) -> usize {
        self.frames.len() - 1
    }

    /// Push a new scope onto the stack
    pub fn push(&mut self, scope_type: ScopeType) {
        let depth = self.frames.len();
        self.frames.push(ScopeFrame::new(scope_type, depth));
    }

    /// Pop the current scope from the stack
    pub fn pop(&mut self) {
        if self.frames.len() > 1 {
            self.frames.pop();
        }
    }

    /// Define a variable in the current scope
    pub fn define_local(&mut self, sym_id: SymbolId, binding_type: BindingType) -> usize {
        let current_frame = self
            .frames
            .last_mut()
            .expect("Should always have at least global scope");
        let index = current_frame.variables.len();
        current_frame.add_variable(sym_id, binding_type, index);
        index
    }

    /// Look up a variable in the scope stack, returning (depth, index) if found
    /// depth = how many scopes up the variable is defined
    /// index = position within that scope
    pub fn lookup(&self, sym_id: SymbolId) -> Option<(usize, usize)> {
        // Search from current frame back to global
        for (i, frame) in self.frames.iter().enumerate().rev() {
            if let Some(binding) = frame.get(sym_id) {
                let depth = self.frames.len() - 1 - i;
                return Some((depth, binding.index));
            }
        }
        None
    }

    /// Check if a variable is defined locally (in current scope)
    pub fn is_defined_local(&self, sym_id: SymbolId) -> bool {
        if let Some(frame) = self.frames.last() {
            frame.contains(sym_id)
        } else {
            false
        }
    }

    /// Get the type of the current scope
    pub fn current_scope_type(&self) -> ScopeType {
        self.frames
            .last()
            .map(|f| f.scope_type)
            .unwrap_or(ScopeType::Global)
    }

    /// Get the number of variables in the current scope
    pub fn current_scope_var_count(&self) -> usize {
        self.frames.last().map(|f| f.variables.len()).unwrap_or(0)
    }

    /// Get a mutable reference to the current scope frame
    pub fn current_frame_mut(&mut self) -> Option<&mut ScopeFrame> {
        self.frames.last_mut()
    }

    /// Get an immutable reference to the current scope frame
    pub fn current_frame(&self) -> Option<&ScopeFrame> {
        self.frames.last()
    }
}

impl Default for CompileScope {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_creation() {
        let scope = CompileScope::new();
        assert_eq!(scope.current_depth(), 0);
        assert_eq!(scope.current_scope_type(), ScopeType::Global);
    }

    #[test]
    fn test_push_pop_scope() {
        let mut scope = CompileScope::new();
        scope.push(ScopeType::Function);
        assert_eq!(scope.current_depth(), 1);
        assert_eq!(scope.current_scope_type(), ScopeType::Function);

        scope.pop();
        assert_eq!(scope.current_depth(), 0);
        assert_eq!(scope.current_scope_type(), ScopeType::Global);
    }

    #[test]
    fn test_define_and_lookup_local() {
        let mut scope = CompileScope::new();
        let sym = SymbolId(42);

        let index = scope.define_local(sym, BindingType::Local);
        assert_eq!(index, 0);

        let result = scope.lookup(sym);
        assert_eq!(result, Some((0, 0)));
    }

    #[test]
    fn test_lookup_from_parent_scope() {
        let mut scope = CompileScope::new();
        let sym1 = SymbolId(1);
        let sym2 = SymbolId(2);

        // Define in global
        scope.define_local(sym1, BindingType::Local);

        // Push function scope and define another var
        scope.push(ScopeType::Function);
        scope.define_local(sym2, BindingType::Parameter);

        // Should find sym2 locally (depth 0)
        assert_eq!(scope.lookup(sym2), Some((0, 0)));

        // Should find sym1 in parent (depth 1)
        assert_eq!(scope.lookup(sym1), Some((1, 0)));
    }

    #[test]
    fn test_is_defined_local() {
        let mut scope = CompileScope::new();
        let sym1 = SymbolId(1);
        let sym2 = SymbolId(2);

        scope.define_local(sym1, BindingType::Local);
        scope.push(ScopeType::Function);
        scope.define_local(sym2, BindingType::Parameter);

        assert!(!scope.is_defined_local(sym1));
        assert!(scope.is_defined_local(sym2));
    }

    #[test]
    fn test_nested_scopes() {
        let mut scope = CompileScope::new();

        let sym1 = SymbolId(1);
        let sym2 = SymbolId(2);
        let sym3 = SymbolId(3);

        scope.define_local(sym1, BindingType::Local);

        scope.push(ScopeType::Function);
        scope.define_local(sym2, BindingType::Parameter);

        scope.push(ScopeType::Block);
        scope.define_local(sym3, BindingType::Local);

        // All should be findable
        assert_eq!(scope.lookup(sym3), Some((0, 0)));
        assert_eq!(scope.lookup(sym2), Some((1, 0)));
        assert_eq!(scope.lookup(sym1), Some((2, 0)));

        // Only sym3 is local
        assert!(scope.is_defined_local(sym3));

        scope.pop();
        assert_eq!(scope.lookup(sym3), None);
        assert_eq!(scope.lookup(sym2), Some((0, 0)));
    }
}
