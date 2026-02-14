use super::ast::Expr;

/// Entry point: resolve captures in the AST
///
/// NOTE: With VarRef-based AST, capture resolution happens during parsing.
/// This function is kept as a no-op for API compatibility.
pub fn resolve_captures(_expr: &mut Expr) {
    // VarRef captures are resolved at parse time - nothing to do here
}
