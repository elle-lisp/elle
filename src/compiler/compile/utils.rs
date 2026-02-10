use super::super::ast::Expr;
use crate::value::SymbolId;

/// Collect all define statements from an expression
/// Returns a vector of symbol IDs that are defined at this level
/// Recursively collects from nested structures like while/for loop bodies
pub fn collect_defines(expr: &Expr) -> Vec<SymbolId> {
    let mut defines = Vec::new();
    let mut seen = std::collections::HashSet::new();

    fn collect_recursive(
        expr: &Expr,
        defines: &mut Vec<SymbolId>,
        seen: &mut std::collections::HashSet<u32>,
    ) {
        match expr {
            Expr::Begin(exprs) => {
                for e in exprs {
                    if let Expr::Define { name, .. } = e {
                        if seen.insert(name.0) {
                            defines.push(*name);
                        }
                    }
                    // Also recursively collect from nested structures
                    // BUT: Don't recurse into nested lambdas (they have their own scope)
                    if !matches!(e, Expr::Lambda { .. }) {
                        collect_recursive(e, defines, seen);
                    }
                }
            }
            Expr::Define { name, .. } => {
                if seen.insert(name.0) {
                    defines.push(*name);
                }
            }
            Expr::While { body, .. } | Expr::For { body, .. } => {
                collect_recursive(body, defines, seen);
            }
            _ => {}
        }
    }

    collect_recursive(expr, &mut defines, &mut seen);
    defines
}
