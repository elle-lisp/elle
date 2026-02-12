mod binding_forms;
mod control_flow;
mod exception_handling;
mod quasiquote;
mod threading;
mod value_to_expr;
mod variable_analysis;

use crate::value::SymbolId;

pub use value_to_expr::value_to_expr;

/// Distinguishes lambda scopes from let scopes for proper variable resolution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeType {
    Function, // Lambda scope - variables in closure environment
    Let,      // Let scope - variables on runtime scope stack
}

/// A scope entry with its type and symbols
#[derive(Debug, Clone)]
pub struct ScopeEntry {
    pub symbols: Vec<SymbolId>,
    pub scope_type: ScopeType,
}
