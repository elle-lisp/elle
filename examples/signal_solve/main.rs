// audited: 2026-10-24
//! A spike: solve signal inference across files, and compare the answer with the analyzer's.
//!
//! docs/impl/solver.md
//! docs/modules-proposal.md
//!
//! Usage: `signal_solve [--all] [--no-follow] [--expect] FILE...`
//!
//! The binary analyzes each FILE and, unless `--no-follow`, every `.lisp`
//! file that its literal imports name. The compiler's extractor states each
//! file's HIR as Datalog facts, and the link joins them through the shapes of
//! the files this run analyzed. The compiler's worklist computes the least
//! model over all of them at once, and datafrog computes it again in
//! crosscheck.rs. The run fails when the two models differ, or when
//! `--expect` finds an `# expect` or `# violates` line in the analyzed files
//! that the model does not meet.

mod crosscheck;
mod expect;
mod report;
mod visit;

#[cfg(test)]
mod tests;

use elle::hir::solve::{self, Extractor, Model, VarKey};
use elle::hir::{Binding, BindingArena, Hir, HirId, LambdaDecl};
use elle::signals::Signal;
use elle::value::fiber::SignalBits;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

/// One analyzed file.
pub struct Unit {
    pub id: u32,
    pub path: String,
    pub hir: Hir,
    pub arena: BindingArena,
    pub decls: HashMap<HirId, LambdaDecl>,
    /// What the code outside every lambda raises by itself.
    pub top_direct: SignalBits,
    /// The signal of each binding the analysis started with.
    pub fixed: HashMap<Binding, Signal>,
}

impl Unit {
    /// The file as the compiler's extractor reads it.
    fn view(&self) -> solve::Unit<'_> {
        solve::Unit {
            id: self.id,
            hir: &self.hir,
            arena: &self.arena,
            decls: &self.decls,
            top_direct: self.top_direct,
            fixed: &self.fixed,
        }
    }
}

fn main() {
    let mut all = false;
    let mut follow = true;
    let mut expect = false;
    let mut files = Vec::new();
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--all" => all = true,
            "--no-follow" => follow = false,
            "--expect" => expect = true,
            _ => files.push(a),
        }
    }
    if files.is_empty() {
        eprintln!("usage: signal_solve [--all] [--no-follow] [--expect] FILE...");
        std::process::exit(2);
    }

    let mut rt = elle::runtime::Runtime::new();
    let t = Instant::now();
    let mut units: Vec<Unit> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();
    let mut queue: VecDeque<String> = files.iter().map(|f| visit::canonical(f)).collect();
    let mut seen: HashSet<String> = queue.iter().cloned().collect();
    while let Some(path) = queue.pop_front() {
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                failed.push((path, e.to_string()));
                continue;
            }
        };
        let (vm, symbols, cctx) = rt.parts();
        let analysis = match elle::pipeline::analyze_file(&source, symbols, vm, cctx, &path) {
            Ok(a) if a.errors.is_empty() => a,
            Ok(a) => {
                failed.push((path, a.errors[0].description()));
                continue;
            }
            Err(e) => {
                failed.push((path, e));
                continue;
            }
        };
        let mut imports = Vec::new();
        visit::literal_imports(&analysis.hir, &analysis.arena, &mut imports);
        if follow {
            for i in imports {
                if seen.insert(i.clone()) {
                    queue.push_back(i);
                }
            }
        }
        units.push(Unit {
            id: units.len() as u32,
            path,
            hir: analysis.hir,
            arena: analysis.arena,
            decls: analysis.lambda_decls,
            top_direct: analysis.top_direct,
            fixed: analysis.fixed,
        });
    }
    let analyze_time = t.elapsed();

    let t = Instant::now();
    let mut model = Model::default();
    let mut shapes = HashMap::new();
    let mut pendings = Vec::new();
    let mut names = HashMap::new();
    for unit in &units {
        let view = unit.view();
        let mut ex = Extractor::new(&mut model, &view);
        let shape = ex.module_shape();
        let top = ex.model.var(VarKey::Top(unit.id));
        ex.walk_file(top);
        shapes.insert(unit.path.clone(), shape);
        let mut pending = std::mem::take(&mut ex.pending);
        // The link keys each module by its file, in the one spelling the
        // shapes use.
        for (_, _, path, _) in &mut pending.instantiations {
            *path = visit::canonical(path);
        }
        for (_, path, _) in &mut pending.instances {
            *path = visit::canonical(path);
        }
        pendings.push(pending);
        for (id, sym) in &ex.lambda_names {
            names.insert((unit.id, id.0), *sym);
        }
    }
    solve::link(&mut model, &shapes, pendings);
    let lowered = solve::lower(&model);
    let extract_time = t.elapsed();
    if std::env::var_os("SIGNAL_SOLVE_PRIMS").is_some() {
        for key in &model.keys_by_id {
            if let VarKey::Fixed(f, b) = key {
                let unit = &units[*f as usize];
                eprintln!(
                    "fixed {} = {:?}",
                    rt.symbols().name(unit.arena.get(*b).name).unwrap_or("?"),
                    unit.fixed.get(b)
                );
            }
        }
    }

    let t = Instant::now();
    let solution = solve::solve(&model, &lowered).solution(&model);
    let solve_time = t.elapsed();
    let t = Instant::now();
    let check = crosscheck::solve(&model, &lowered);
    let check_time = t.elapsed();

    let symbols = rt.symbols();
    let ctx = report::Context {
        model: &model,
        solution: &solution,
        units: &units,
        names: &names,
        symbols,
        shapes: &shapes,
    };
    let failures = report::print(
        &ctx,
        &report::Run {
            all,
            failed: &failed,
            analyze_time,
            extract_time,
            solve_time,
            check_time,
            expect,
        },
    );
    let mut exit = 0;
    if check != solution {
        println!("the worklist and datafrog solved different models:");
        let n = report::disagreement(&ctx, &check);
        eprintln!("the two engines disagree on {n} tuple(s)");
        exit = 1;
    }
    if failures > 0 {
        eprintln!("{failures} expectation(s) failed");
        exit = 1;
    }
    std::process::exit(exit);
}
