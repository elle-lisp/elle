//! The evaluation-order tree walk that carries nonzero-divisor path facts and
//! dispatches each call-position `%`-intrinsic to its per-site contract check.

use super::*;

pub(super) fn walk(
    h: &Hir,
    hir_types: &HashMap<HirId, TyId>,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
    interner: &TypeInterner,
    env: &mut NonzeroEnv,
) -> Result<(), String> {
    macro_rules! recurse {
        ($e:expr, $env:expr) => {
            walk($e, hir_types, arena, symbol_names, interner, $env)
        };
    }

    match &h.kind {
        HirKind::Intrinsic { op, args } => {
            for a in args {
                recurse!(a, env)?;
            }
            let arg_refs: Vec<&Hir> = args.iter().collect();
            check_op(*op, &arg_refs, h, hir_types, interner, env)
        }
        HirKind::Call { func, args, .. } => {
            recurse!(func, env)?;
            for a in args {
                recurse!(&a.expr, env)?;
            }
            // A call whose callee is the %-named NativeFn is the storing ops'
            // call-position form — same proof, native lowering. A spliced
            // argument list has no per-operand types to check; the native's
            // own runtime validation covers that (dynamic) shape.
            if let HirKind::Var(b) = &func.kind {
                if let Some(name) = symbol_names.get(&arena.get(*b).name.0) {
                    if name.starts_with('%') && !args.iter().any(|a| a.spliced) {
                        if let Some(op) = IntrinsicOp::from_name(name) {
                            let arg_refs: Vec<&Hir> = args.iter().map(|a| &a.expr).collect();
                            return check_op(op, &arg_refs, h, hir_types, interner, env);
                        }
                    }
                }
            }
            Ok(())
        }
        // Statement sequences: a diverging one-armed guard leaves its
        // fall-through facts standing for the rest of the sequence.
        HirKind::Begin(exprs) => {
            let saved = env.clone();
            for e in exprs {
                recurse!(e, env)?;
                env.apply(&guard::facts_after_statement(e, arena, symbol_names));
            }
            *env = saved;
            Ok(())
        }
        HirKind::Block { body, .. } => {
            let saved = env.clone();
            for e in body {
                recurse!(e, env)?;
                env.apply(&guard::facts_after_statement(e, arena, symbol_names));
            }
            *env = saved;
            Ok(())
        }
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            recurse!(cond, env)?;
            let facts = guard::cond_facts(cond, arena, symbol_names);
            let mut then_env = env.clone();
            then_env.apply(&facts.when_true);
            recurse!(then_branch, &mut then_env)?;
            let mut else_env = env.clone();
            else_env.apply(&facts.when_false);
            recurse!(else_branch, &mut else_env)?;
            // A diverging branch leaves the other branch's facts standing.
            if guard::diverges(then_branch) {
                env.apply(&facts.when_false);
            }
            if guard::diverges(else_branch) {
                env.apply(&facts.when_true);
            }
            Ok(())
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            // Sequential If chain: each body gets its test's truthy facts;
            // later clauses (and the else) know every earlier test was falsy.
            // The falsy accumulation is scoped to the Cond — after it, some
            // clause may have run, so none of the falsy facts hold outside.
            let mut running = env.clone();
            for (test, body) in clauses {
                recurse!(test, &mut running)?;
                let facts = guard::cond_facts(test, arena, symbol_names);
                let mut body_env = running.clone();
                body_env.apply(&facts.when_true);
                recurse!(body, &mut body_env)?;
                running.apply(&facts.when_false);
            }
            if let Some(els) = else_branch {
                recurse!(els, &mut running)?;
            }
            Ok(())
        }
        HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
            for (b, init) in bindings {
                recurse!(init, env)?;
                // A binding initialized from a nonzero literal is itself proven.
                match &init.kind {
                    HirKind::Int(n) if *n != 0 => env.insert(*b),
                    HirKind::Float(f) if *f != 0.0 => env.insert(*b),
                    _ => false,
                };
            }
            recurse!(body, env)
        }
        // Mutation invalidates a proven fact.
        HirKind::Assign { target, value }
        | HirKind::Define {
            binding: target,
            value,
        } => {
            recurse!(value, env)?;
            env.invalidate(*target);
            Ok(())
        }
        HirKind::SetCell { cell, value } => {
            recurse!(cell, env)?;
            recurse!(value, env)?;
            if let HirKind::Var(b) = &cell.kind {
                env.invalidate(*b);
            }
            Ok(())
        }
        // A loop body re-enters: any binding it mutates is unproven for the
        // whole body (the back edge would carry the mutated value into a use
        // textually before the mutation).
        HirKind::Loop { bindings, body } => {
            for (_, init) in bindings {
                recurse!(init, env)?;
            }
            let mut body_env = env.clone();
            for b in collect_mutated(body) {
                body_env.invalidate(b);
            }
            recurse!(body, &mut body_env)
        }
        // A lambda body runs at unknown times relative to the surrounding
        // flow; it starts with no path facts.
        HirKind::Lambda { body, .. } => {
            let mut fresh = NonzeroEnv::default();
            recurse!(body, &mut fresh)
        }
        HirKind::Match { value, arms } => {
            recurse!(value, env)?;
            for (_, arm_guard, arm_body) in arms {
                let mut arm_env = env.clone();
                if let Some(g) = arm_guard {
                    recurse!(g, &mut arm_env)?;
                }
                recurse!(arm_body, &mut arm_env)?;
            }
            Ok(())
        }
        _ => {
            let mut result = Ok(());
            h.for_each_child(|c| {
                if result.is_ok() {
                    result = recurse!(c, env);
                }
            });
            result
        }
    }
}

/// Bindings mutated anywhere inside `h` (Assign / Define / SetCell targets).
fn collect_mutated(h: &Hir) -> Vec<Binding> {
    let mut out = Vec::new();
    fn go(h: &Hir, out: &mut Vec<Binding>) {
        match &h.kind {
            HirKind::Assign { target, .. }
            | HirKind::Define {
                binding: target, ..
            } => {
                out.push(*target);
            }
            HirKind::SetCell { cell, .. } => {
                if let HirKind::Var(b) = &cell.kind {
                    out.push(*b);
                }
            }
            _ => {}
        }
        h.for_each_child(|c| go(c, out));
    }
    go(h, &mut out);
    out
}
