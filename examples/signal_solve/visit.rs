// audited: 2026-11-09
//! Find the `.lisp` files a file's literal imports name, in the one spelling the link keys on.
//!
//! docs/impl/solver.md

use elle::hir::{BindingArena, Hir, HirKind};
use elle::value::SymbolId;
use std::path::Path;

/// A path in the one spelling every table keys on.
pub fn canonical(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}

/// The file a literal load in the unit `file` names, in canonical spelling. A
/// relative path names a file beside the unit, as a fixture expects.
pub fn beside(file: &str, path: &str) -> String {
    let dir = Path::new(file).parent().unwrap_or(Path::new(""));
    canonical(&dir.join(path).to_string_lossy())
}

/// The `.lisp` paths of every literal `((import/load-file "path"))` in the
/// unit `file`. A library loads through `import/load-plugin` instead.
pub fn literal_imports(hir: &Hir, arena: &BindingArena, file: &str, out: &mut Vec<String>) {
    if let HirKind::Call { func, args, .. } = &hir.kind {
        let is_load = matches!(&func.kind, HirKind::Var(b)
            if arena.get(*b).is_primitive && arena.get(*b).name == SymbolId::of("import/load-file"));
        if is_load {
            if let Some(HirKind::String(literal)) = args.first().map(|a| &a.expr.kind) {
                if literal.ends_with(".lisp") {
                    let path = beside(file, literal);
                    if !out.contains(&path) {
                        out.push(path);
                    }
                }
            }
        }
    }
    hir.for_each_child(|c| literal_imports(c, arena, file, out));
}
