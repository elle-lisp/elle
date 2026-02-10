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
