//! Pretty-print HIR as s-expressions.
//!
//! Used by `--dump=fhir` to show the functionalized HIR before lowering.

use super::arena::BindingArena;
use super::binding::Binding;
use super::expr::{Hir, HirKind};
use std::collections::HashMap;
use std::fmt::Write;

/// Pretty-print a HIR tree as an s-expression string.
pub fn display_hir(hir: &Hir, arena: &BindingArena, names: &HashMap<u32, String>) -> String {
    let mut buf = String::new();
    write_hir(&mut buf, hir, arena, names, 0);
    buf
}

fn binding_name(b: Binding, arena: &BindingArena, names: &HashMap<u32, String>) -> String {
    let sym = arena.get(b).name;
    let base = names
        .get(&sym.0)
        .cloned()
        .unwrap_or_else(|| format!("_{}", b.0));
    // Append binding ID to disambiguate SSA versions
    format!("{}#{}", base, b.0)
}

fn indent(buf: &mut String, depth: usize) {
    for _ in 0..depth {
        buf.push_str("  ");
    }
}

fn write_hir(
    buf: &mut String,
    hir: &Hir,
    arena: &BindingArena,
    names: &HashMap<u32, String>,
    depth: usize,
) {
    match &hir.kind {
        HirKind::Nil => buf.push_str("nil"),
        HirKind::EmptyList => buf.push_str("()"),
        HirKind::Bool(true) => buf.push_str("true"),
        HirKind::Bool(false) => buf.push_str("false"),
        HirKind::Int(n) => write!(buf, "{}", n).unwrap(),
        HirKind::Float(f) => write!(buf, "{}", f).unwrap(),
        HirKind::String(s) => write!(buf, "\"{}\"", s.replace('"', "\\\"")).unwrap(),
        HirKind::Keyword(k) => write!(buf, ":{}", k).unwrap(),
        HirKind::Quote(v) => write!(buf, "'{}", v).unwrap(),
        HirKind::Error => buf.push_str("<error>"),

        HirKind::Var(b) => {
            buf.push_str(&binding_name(*b, arena, names));
        }

        HirKind::Let { bindings, body } => {
            buf.push_str("(let [");
            for (i, (b, init)) in bindings.iter().enumerate() {
                if i > 0 {
                    buf.push('\n');
                    indent(buf, depth + 3);
                }
                buf.push_str(&binding_name(*b, arena, names));
                buf.push(' ');
                write_hir(buf, init, arena, names, depth + 3);
            }
            buf.push_str("]\n");
            indent(buf, depth + 1);
            write_hir(buf, body, arena, names, depth + 1);
            buf.push(')');
        }

        HirKind::Letrec { bindings, body } => {
            buf.push_str("(letrec [");
            for (i, (b, init)) in bindings.iter().enumerate() {
                if i > 0 {
                    buf.push('\n');
                    indent(buf, depth + 5);
                }
                buf.push_str(&binding_name(*b, arena, names));
                buf.push(' ');
                write_hir(buf, init, arena, names, depth + 5);
            }
            buf.push_str("]\n");
            indent(buf, depth + 1);
            write_hir(buf, body, arena, names, depth + 1);
            buf.push(')');
        }

        HirKind::Lambda {
            params,
            rest_param,
            captures,
            body,
            ..
        } => {
            buf.push_str("(fn [");
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    buf.push(' ');
                }
                buf.push_str(&binding_name(*p, arena, names));
            }
            if let Some(rp) = rest_param {
                buf.push_str(" & ");
                buf.push_str(&binding_name(*rp, arena, names));
            }
            buf.push(']');
            if !captures.is_empty() {
                buf.push_str(" ;captures=[");
                for (i, c) in captures.iter().enumerate() {
                    if i > 0 {
                        buf.push_str(", ");
                    }
                    buf.push_str(&binding_name(c.binding, arena, names));
                }
                buf.push(']');
            }
            buf.push('\n');
            indent(buf, depth + 1);
            write_hir(buf, body, arena, names, depth + 1);
            buf.push(')');
        }

        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            buf.push_str("(if ");
            write_hir(buf, cond, arena, names, depth + 1);
            buf.push('\n');
            indent(buf, depth + 2);
            write_hir(buf, then_branch, arena, names, depth + 2);
            buf.push('\n');
            indent(buf, depth + 2);
            write_hir(buf, else_branch, arena, names, depth + 2);
            buf.push(')');
        }

        HirKind::Begin(exprs) => {
            buf.push_str("(begin");
            for e in exprs {
                buf.push('\n');
                indent(buf, depth + 1);
                write_hir(buf, e, arena, names, depth + 1);
            }
            buf.push(')');
        }

        HirKind::Block { name, body, .. } => {
            buf.push_str("(block");
            if let Some(n) = name {
                write!(buf, " :{}", n).unwrap();
            }
            for e in body {
                buf.push('\n');
                indent(buf, depth + 1);
                write_hir(buf, e, arena, names, depth + 1);
            }
            buf.push(')');
        }

        HirKind::Break { value, .. } => {
            buf.push_str("(break ");
            write_hir(buf, value, arena, names, depth + 1);
            buf.push(')');
        }

        HirKind::Call {
            func,
            args,
            is_tail,
        } => {
            buf.push('(');
            if *is_tail {
                buf.push_str("/*tail*/ ");
            }
            write_hir(buf, func, arena, names, depth + 1);
            for a in args {
                buf.push(' ');
                if a.spliced {
                    buf.push(';');
                }
                write_hir(buf, &a.expr, arena, names, depth + 1);
            }
            buf.push(')');
        }

        HirKind::Assign { target, value } => {
            buf.push_str("(assign ");
            buf.push_str(&binding_name(*target, arena, names));
            buf.push(' ');
            write_hir(buf, value, arena, names, depth + 1);
            buf.push(')');
        }

        HirKind::Define { binding, value } => {
            buf.push_str("(define ");
            buf.push_str(&binding_name(*binding, arena, names));
            buf.push(' ');
            write_hir(buf, value, arena, names, depth + 1);
            buf.push(')');
        }

        HirKind::While { cond, body } => {
            buf.push_str("(while ");
            write_hir(buf, cond, arena, names, depth + 1);
            buf.push('\n');
            indent(buf, depth + 1);
            write_hir(buf, body, arena, names, depth + 1);
            buf.push(')');
        }

        HirKind::Loop { bindings, body } => {
            buf.push_str("(loop [");
            for (i, (b, init)) in bindings.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                buf.push_str(&binding_name(*b, arena, names));
                buf.push(' ');
                write_hir(buf, init, arena, names, depth + 3);
            }
            buf.push_str("]\n");
            indent(buf, depth + 1);
            write_hir(buf, body, arena, names, depth + 1);
            buf.push(')');
        }

        HirKind::Recur { args } => {
            buf.push_str("(recur");
            for a in args {
                buf.push(' ');
                write_hir(buf, a, arena, names, depth + 1);
            }
            buf.push(')');
        }

        HirKind::MakeCell { value } => {
            buf.push_str("(make-cell ");
            write_hir(buf, value, arena, names, depth + 1);
            buf.push(')');
        }

        HirKind::DerefCell { cell } => {
            buf.push_str("(deref-cell ");
            write_hir(buf, cell, arena, names, depth + 1);
            buf.push(')');
        }

        HirKind::SetCell { cell, value } => {
            buf.push_str("(set-cell ");
            write_hir(buf, cell, arena, names, depth + 1);
            buf.push(' ');
            write_hir(buf, value, arena, names, depth + 1);
            buf.push(')');
        }

        HirKind::Emit { signal, value } => {
            write!(buf, "(emit {:?} ", signal).unwrap();
            write_hir(buf, value, arena, names, depth + 1);
            buf.push(')');
        }

        HirKind::Match { value, arms } => {
            buf.push_str("(match ");
            write_hir(buf, value, arena, names, depth + 1);
            for (pat, guard, body) in arms {
                buf.push('\n');
                indent(buf, depth + 1);
                write!(buf, "{:?}", pat).unwrap();
                if let Some(g) = guard {
                    buf.push_str(" when ");
                    write_hir(buf, g, arena, names, depth + 2);
                }
                buf.push('\n');
                indent(buf, depth + 2);
                write_hir(buf, body, arena, names, depth + 2);
            }
            buf.push(')');
        }

        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            buf.push_str("(cond");
            for (c, b) in clauses {
                buf.push('\n');
                indent(buf, depth + 1);
                write_hir(buf, c, arena, names, depth + 1);
                buf.push('\n');
                indent(buf, depth + 2);
                write_hir(buf, b, arena, names, depth + 2);
            }
            if let Some(e) = else_branch {
                buf.push('\n');
                indent(buf, depth + 1);
                buf.push_str("else\n");
                indent(buf, depth + 2);
                write_hir(buf, e, arena, names, depth + 2);
            }
            buf.push(')');
        }

        HirKind::And(exprs) => {
            buf.push_str("(and");
            for e in exprs {
                buf.push(' ');
                write_hir(buf, e, arena, names, depth + 1);
            }
            buf.push(')');
        }

        HirKind::Or(exprs) => {
            buf.push_str("(or");
            for e in exprs {
                buf.push(' ');
                write_hir(buf, e, arena, names, depth + 1);
            }
            buf.push(')');
        }

        HirKind::Destructure { pattern, value, .. } => {
            write!(buf, "(destructure {:?} ", pattern).unwrap();
            write_hir(buf, value, arena, names, depth + 1);
            buf.push(')');
        }

        HirKind::Eval { expr, env } => {
            buf.push_str("(eval ");
            write_hir(buf, expr, arena, names, depth + 1);
            buf.push(' ');
            write_hir(buf, env, arena, names, depth + 1);
            buf.push(')');
        }

        HirKind::Parameterize { bindings, body } => {
            buf.push_str("(parameterize [");
            for (i, (k, v)) in bindings.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                buf.push('(');
                write_hir(buf, k, arena, names, depth + 1);
                buf.push(' ');
                write_hir(buf, v, arena, names, depth + 1);
                buf.push(')');
            }
            buf.push_str("]\n");
            indent(buf, depth + 1);
            write_hir(buf, body, arena, names, depth + 1);
            buf.push(')');
        }

        HirKind::Intrinsic { op, args } => {
            write!(buf, "({}", op.name()).unwrap();
            for a in args {
                buf.push(' ');
                write_hir(buf, a, arena, names, depth + 1);
            }
            buf.push(')');
        }
    }
}
