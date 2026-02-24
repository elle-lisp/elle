//! Binding types for HIR
//!
//! A `Binding` is a NaN-boxed Value pointing to a HeapObject::Binding on the
//! heap. Identity is bit-pattern equality (same heap pointer = same binding).
//! Binding is Copy (8 bytes). The underlying BindingInner is mutable via
//! RefCell during analysis; the lowerer only reads.
//!
//! Binding must only be constructed via `Binding::new()`.
//!
//! Each `Binding::new()` leaks an `Rc<HeapObject>` (via `Rc::into_raw` in
//! `alloc()`). This is the same pattern used for all NaN-boxed heap values.
//! The leak is bounded by the number of binding sites per compilation unit.

use crate::value::heap::{BindingInner, BindingScope};
use crate::value::{SymbolId, Value};
use std::fmt;
use std::hash::{Hash, Hasher};

/// A binding handle wrapping a NaN-boxed Value.
#[derive(Clone, Copy)]
pub struct Binding(Value);

impl Binding {
    /// Create a new binding
    pub fn new(name: SymbolId, scope: BindingScope) -> Self {
        Binding(Value::binding(name, scope))
    }

    /// Get the inner RefCell for direct access
    fn inner(&self) -> &std::cell::RefCell<BindingInner> {
        self.0.as_binding().expect("Binding holds a binding Value")
    }

    // Read accessors
    pub fn name(&self) -> SymbolId {
        self.inner().borrow().name
    }
    pub fn scope(&self) -> BindingScope {
        self.inner().borrow().scope
    }
    pub fn is_mutated(&self) -> bool {
        self.inner().borrow().is_mutated
    }
    pub fn is_captured(&self) -> bool {
        self.inner().borrow().is_captured
    }
    pub fn is_immutable(&self) -> bool {
        self.inner().borrow().is_immutable
    }
    pub fn is_global(&self) -> bool {
        self.scope() == BindingScope::Global
    }

    /// A binding needs a cell if captured (for locals) or mutated (for params)
    pub fn needs_cell(&self) -> bool {
        let inner = self.inner().borrow();
        match inner.scope {
            BindingScope::Local => inner.is_captured,
            BindingScope::Parameter => inner.is_mutated,
            BindingScope::Global => false,
        }
    }

    // Mutation (used by analyzer during analysis only)
    pub fn mark_mutated(&self) {
        self.inner().borrow_mut().is_mutated = true;
    }
    pub fn mark_captured(&self) {
        self.inner().borrow_mut().is_captured = true;
    }
    pub fn mark_immutable(&self) {
        self.inner().borrow_mut().is_immutable = true;
    }
}

impl PartialEq for Binding {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for Binding {}

impl Hash for Binding {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

impl fmt::Debug for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = self.inner().borrow();
        write!(f, "Binding({:?}, {:?})", inner.name, inner.scope)
    }
}

/// Information about a captured variable in a closure
#[derive(Debug, Clone)]
pub struct CaptureInfo {
    /// The binding being captured
    pub binding: Binding,
    /// How to access this capture from the parent scope
    pub kind: CaptureKind,
}

/// How a capture is accessed from the enclosing scope
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureKind {
    /// Capture from parent's local slot (resolved by lowerer via binding_to_slot)
    Local,
    /// Capture from parent's capture (transitive capture)
    Capture { index: u16 },
    /// Capture from global scope
    Global { sym: SymbolId },
}
