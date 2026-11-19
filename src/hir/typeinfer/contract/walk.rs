// audited: 2026-11-18
//! The walk that checks each call-position `%`-intrinsic in evaluation order, carrying nonzero-divisor facts.
//!
//! docs/intrinsics.md
//! docs/impl/typeinfer/facts.md
//! docs/impl/graft.md

use super::*;

/// Check every site in `h`. `env` holds the nonzero facts in force, and
/// `captured` what the innermost lambda the walk stands in captures, which a
/// call or an `emit` there may let the owner write.
pub(super) fn walk(
    h: &Hir,
    hir_types: &NodeTypes,
    arena: &BindingArena,
    interner: &TypeInterner,
    env: &mut NonzeroEnv,
    captured: &FxHashSet<Binding>,
) -> Result<(), String> {
    macro_rules! recurse {
        ($e:expr, $env:expr) => {
            walk($e, hir_types, arena, interner, $env, captured)
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
            // A call to a `%`-named native is the call-position form of an op
            // that lowers through the native: the same proof applies. A
            // spliced argument list has no per-operand types to check, and the
            // native validates it at run time.
            if let HirKind::Var(b) = &func.kind {
                if let Some(name) = crate::primitives::registration::static_name(arena.get(*b).name)
                {
                    if name.starts_with('%') && !args.iter().any(|a| a.spliced) {
                        if let Some(op) = IntrinsicOp::from_name(name) {
                            let arg_refs: Vec<&Hir> = args.iter().map(|a| &a.expr).collect();
                            check_op(op, &arg_refs, h, hir_types, interner, env)?;
                        }
                    }
                }
            }
            env.end_shared(arena, captured);
            Ok(())
        }
        // The code that runs while the fiber is parked may write a shared
        // binding, as a call's callee may.
        HirKind::Emit { value, .. } => {
            recurse!(value, env)?;
            env.end_shared(arena, captured);
            Ok(())
        }
        // Statement sequences: a guard with one diverging branch leaves its
        // fall-through facts standing for the rest of the sequence. A
        // statement that diverges ends the check: only a resume of its raise
        // reaches what follows, over operands no type proves. The facts the
        // sequence added end with it, and the facts its writes ended stay
        // ended.
        HirKind::Begin(exprs) => {
            let mut saved = env.clone();
            walk_sequence(exprs, hir_types, arena, interner, env, captured)?;
            saved.intersect(env);
            *env = saved;
            Ok(())
        }
        // A `break` leaves a block from any statement, so after it no fact
        // holds about a binding the block writes.
        HirKind::Block { body, .. } => {
            let mut saved = env.clone();
            walk_sequence(body, hir_types, arena, interner, env, captured)?;
            saved.intersect(env);
            *env = saved;
            for exprs in body {
                for b in collect_mutated(exprs) {
                    env.invalidate(b);
                }
            }
            Ok(())
        }
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            recurse!(cond, env)?;
            let facts = guard::cond_facts(cond, arena);
            let mut then_env = env.clone();
            then_env.apply(&facts.when_true);
            recurse!(then_branch, &mut then_env)?;
            let mut else_env = env.clone();
            else_env.apply(&facts.when_false);
            recurse!(else_branch, &mut else_env)?;
            // A diverging branch leaves the other branch's facts standing, and
            // a write in either branch ends its fact after the `if`.
            let paths = falls(then_branch, then_env)
                .into_iter()
                .chain(falls(else_branch, else_env))
                .collect();
            env.meet(paths);
            Ok(())
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            // Sequential If chain: each body gets its test's truthy facts;
            // later clauses (and the else) know every earlier test was falsy.
            let mut running = env.clone();
            let mut paths = Vec::new();
            for (test, body) in clauses {
                recurse!(test, &mut running)?;
                let facts = guard::cond_facts(test, arena);
                let mut body_env = running.clone();
                body_env.apply(&facts.when_true);
                recurse!(body, &mut body_env)?;
                paths.extend(falls(body, body_env));
                running.apply(&facts.when_false);
            }
            // With no else, the path where every test was falsy falls through.
            match else_branch {
                Some(els) => {
                    recurse!(els, &mut running)?;
                    paths.extend(falls(els, running));
                }
                None => paths.push(running),
            }
            env.meet(paths);
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
        // textually before the mutation), and after the loop.
        HirKind::Loop { bindings, body } => {
            for (_, init) in bindings {
                recurse!(init, env)?;
            }
            let mutated = collect_mutated(body);
            let mut body_env = env.clone();
            for b in &mutated {
                body_env.invalidate(*b);
                env.invalidate(*b);
            }
            recurse!(body, &mut body_env)
        }
        // A lambda body runs at unknown times relative to the surrounding
        // flow; it starts with no path facts.
        HirKind::Lambda(lambda) => {
            let mut fresh = NonzeroEnv::default();
            let inner: FxHashSet<Binding> = lambda.captures.iter().map(|c| c.binding).collect();
            walk(&lambda.body, hir_types, arena, interner, &mut fresh, &inner)
        }
        HirKind::Match { value, arms } => {
            recurse!(value, env)?;
            let mut paths = Vec::with_capacity(arms.len());
            for (_, arm_guard, arm_body) in arms {
                let mut arm_env = env.clone();
                if let Some(g) = arm_guard {
                    recurse!(g, &mut arm_env)?;
                }
                recurse!(arm_body, &mut arm_env)?;
                paths.extend(falls(arm_body, arm_env));
            }
            env.meet(paths);
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

/// Walk a sequence's statements in order, applying the fall-through facts of
/// each guard, and stop at the first that diverges.
fn walk_sequence(
    exprs: &[Hir],
    hir_types: &NodeTypes,
    arena: &BindingArena,
    interner: &TypeInterner,
    env: &mut NonzeroEnv,
    captured: &FxHashSet<Binding>,
) -> Result<(), String> {
    for e in exprs {
        walk(e, hir_types, arena, interner, env, captured)?;
        if guard::diverges(e) {
            break;
        }
        env.apply(&guard::facts_after_statement(e, arena));
    }
    Ok(())
}

/// The facts at the end of `arm`, when the arm falls through to what follows
/// its form. An arm that diverges reaches nothing after the form.
fn falls(arm: &Hir, env: NonzeroEnv) -> Option<NonzeroEnv> {
    (!guard::diverges(arm)).then_some(env)
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
