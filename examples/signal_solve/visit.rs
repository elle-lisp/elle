// audited: 2026-10-24
//! Find the `.lisp` files a file's literal imports name, in the one spelling the link keys on.
//!
//! docs/impl/solver.md

use elle::hir::{BindingArena, Hir, HirKind};
use elle::value::SymbolId;

/// A path in the one spelling every table keys on.
pub fn canonical(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}

/// The `.lisp` file a literal load names, in canonical spelling. A literal
/// `import-file` path reaches the HIR already absolute, as the argument of
/// `import/load-file`; a library loads through `import/load-plugin` instead.
pub fn resolve(path: &str) -> Option<String> {
    path.ends_with(".lisp").then(|| canonical(path))
}

/// The `.lisp` paths of every literal `((import-file "path"))` in a file.
pub fn literal_imports(hir: &Hir, arena: &BindingArena, out: &mut Vec<String>) {
    if let HirKind::Call { func, args, .. } = &hir.kind {
        let is_load = matches!(&func.kind, HirKind::Var(b)
            if arena.get(*b).is_primitive && arena.get(*b).name == SymbolId::of("import/load-file"));
        if is_load {
            if let Some(HirKind::String(literal)) = args.first().map(|a| &a.expr.kind) {
                if let Some(path) = resolve(literal) {
                    if !out.contains(&path) {
                        out.push(path);
                    }
                }
            }
        }
    }
    hir.for_each_child(|c| literal_imports(c, arena, out));
}
