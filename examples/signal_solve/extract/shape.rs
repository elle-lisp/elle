// audited: 2026-10-05
//! The shape of the value a module file returns, and the exports its struct literal names.
//!
//! docs/impl/solver.md
//! docs/modules-proposal.md

use super::Extractor;
use crate::model::VarId;
use elle::hir::{Hir, HirKind};

/// The shape a module file returns.
#[derive(Debug, Clone)]
pub enum ModuleShape {
    /// A lambda, with the export struct its body ends in.
    Lambda {
        lam: VarId,
        exports: Vec<(String, VarId)>,
    },
    /// A struct literal.
    Struct {
        exports: Vec<(String, VarId)>,
    },
    Unknown,
}

impl<'a> Extractor<'a> {
    /// The shape this file's module value has, and its exports.
    pub fn module_shape(&mut self) -> ModuleShape {
        let unit = self.unit;
        let HirKind::Letrec { bindings, .. } = &unit.hir.kind else {
            return ModuleShape::Unknown;
        };
        let Some((_, last)) = bindings.last() else {
            return ModuleShape::Unknown;
        };
        match &last.kind {
            HirKind::Lambda { body, .. } => {
                let lam = self.lam_var(last.id);
                let mut exports = Vec::new();
                self.export_struct(body, &mut exports);
                ModuleShape::Lambda { lam, exports }
            }
            _ => {
                let mut exports = Vec::new();
                self.export_struct(last, &mut exports);
                if exports.is_empty() {
                    ModuleShape::Unknown
                } else {
                    ModuleShape::Struct { exports }
                }
            }
        }
    }

    /// Collect the fields of the struct literal an expression ends in, through
    /// the forms `compute_signal_projection` unwraps.
    fn export_struct(&mut self, hir: &'a Hir, out: &mut Vec<(String, VarId)>) {
        match &hir.kind {
            HirKind::Call { func, args, .. } if self.is_prim_named(func, "struct") => {
                for pair in args.chunks(2) {
                    if let [k, v] = pair {
                        if let HirKind::Keyword(k) = &k.expr.kind {
                            let a = self.aval(&v.expr);
                            let var = self.var_of(a);
                            out.push((k.clone(), var));
                        }
                    }
                }
            }
            HirKind::Lambda { body, .. } => self.export_struct(body, out),
            HirKind::Begin(exprs) => {
                if let Some(e) = exprs.last() {
                    self.export_struct(e, out);
                }
            }
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                self.export_struct(body, out)
            }
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.export_struct(then_branch, out);
                self.export_struct(else_branch, out);
            }
            HirKind::Return { value } => self.export_struct(value, out),
            _ => {}
        }
    }
}
