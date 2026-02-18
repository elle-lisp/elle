pub mod handlers;
mod runtime_scope;
mod scope_stack;

pub use handlers::{
    handle_define_local, handle_make_cell, handle_pop_scope, handle_push_scope, handle_unwrap_cell,
    handle_update_cell,
};
pub use runtime_scope::RuntimeScope;
pub use scope_stack::ScopeStack;

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
