//! Binding resolution types for HIR

use crate::value::SymbolId;

/// Unique identifier for a binding, assigned during analysis.
/// Unlike SymbolId, this is unique per binding site - two `let x` in
/// different scopes get different BindingIds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindingId(pub u32);

impl BindingId {
    /// Create a new binding ID
    pub fn new(id: u32) -> Self {
        BindingId(id)
    }
}

/// Information about a binding
#[derive(Debug, Clone)]
pub struct BindingInfo {
    /// The unique binding ID
    pub id: BindingId,
    /// Original symbol name (for debugging/error messages)
    pub name: SymbolId,
    /// Whether this binding is mutated (via set!)
    pub is_mutated: bool,
    /// Whether this binding is captured by a nested closure
    pub is_captured: bool,
    /// Whether this binding is immutable (def)
    pub is_immutable: bool,
    /// The kind of binding
    pub kind: BindingKind,
}

/// What kind of binding this is
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingKind {
    /// Lambda parameter
    Parameter { index: u16 },
    /// Let-bound variable
    Local { index: u16 },
    /// Global/top-level definition
    Global,
}

/// Information about a captured variable in a closure
#[derive(Debug, Clone)]
pub struct CaptureInfo {
    /// The binding being captured
    pub binding: BindingId,
    /// How to access this capture from the parent scope
    pub kind: CaptureKind,
    /// Whether the captured variable is mutated (requires cell boxing)
    pub is_mutated: bool,
}

/// How a capture is accessed from the enclosing scope
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureKind {
    /// Capture from parent's local slot
    Local { index: u16 },
    /// Capture from parent's capture (transitive capture)
    Capture { index: u16 },
    /// Capture from global scope
    Global { sym: SymbolId },
}

impl BindingInfo {
    /// Create a new parameter binding
    pub fn parameter(id: BindingId, name: SymbolId, index: u16) -> Self {
        BindingInfo {
            id,
            name,
            is_mutated: false,
            is_captured: false,
            is_immutable: false,
            kind: BindingKind::Parameter { index },
        }
    }

    /// Create a new local binding
    pub fn local(id: BindingId, name: SymbolId, index: u16) -> Self {
        BindingInfo {
            id,
            name,
            is_mutated: false,
            is_captured: false,
            is_immutable: false,
            kind: BindingKind::Local { index },
        }
    }

    /// Create a new global binding
    pub fn global(id: BindingId, name: SymbolId) -> Self {
        BindingInfo {
            id,
            name,
            is_mutated: false,
            is_captured: false,
            is_immutable: false,
            kind: BindingKind::Global,
        }
    }

    /// Mark this binding as mutated
    pub fn mark_mutated(&mut self) {
        self.is_mutated = true;
    }

    /// Mark this binding as captured
    pub fn mark_captured(&mut self) {
        self.is_captured = true;
    }

    /// Check if this binding needs to be boxed in a cell
    /// A binding needs a cell if:
    /// - It's mutated AND captured (standard case for mutable captures)
    /// - OR it's captured AND it's a Local (for letrec-style recursive bindings)
    /// - OR it's a Parameter that's mutated (even if not captured, for StoreCapture to work)
    pub fn needs_cell(&self) -> bool {
        match self.kind {
            BindingKind::Local { .. } => self.is_captured, // Locals need cell if captured (for letrec semantics)
            BindingKind::Parameter { .. } => self.is_mutated, // Params need cell if mutated (for set! to work)
            BindingKind::Global => false,                     // Globals don't use cells
        }
    }
}
