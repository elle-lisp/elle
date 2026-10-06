// audited: 2026-10-05
//! A spike: solve signal inference across files, and compare the answer with the analyzer's.
//!
//! docs/impl/solver.md
//! docs/modules-proposal.md
//!
//! Usage: `signal_solve [--all] [--no-follow] [--expect] FILE...`
//!
//! The binary analyzes each FILE and, unless `--no-follow`, every `.lisp`
//! file that its literal imports name. Each file's HIR becomes Datalog facts.
//! The worklist in fixpoint.rs computes the least model over all of them at
//! once, and datafrog computes it again in crosscheck.rs. The run fails when
//! the two models differ, or when `--expect` finds an `# expect` or
//! `# violates` line in the analyzed files that the model does not meet.

mod crosscheck;
mod expect;
mod extract;
mod fixpoint;
mod link;
mod model;
mod report;
mod visit;

use extract::{Extractor, Unit};
use model::{Model, VarKey};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

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
        let analysis =
            match elle::pipeline::analyze_file_detached(&source, symbols, vm, cctx, &path) {
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
        });
    }
    let analyze_time = t.elapsed();

    let meta = rt.parts().2.meta().signals.clone();
    let t = Instant::now();
    let mut model = Model::default();
    let mut shapes = HashMap::new();
    let mut pendings = Vec::new();
    let mut names = HashMap::new();
    for unit in &units {
        let mut ex = Extractor::new(&mut model, &meta, unit);
        let shape = ex.module_shape();
        let top = ex.model.var(VarKey::Top(unit.id));
        ex.walk(&unit.hir, top);
        shapes.insert(unit.path.clone(), shape);
        pendings.push(std::mem::take(&mut ex.pending));
        for (id, sym) in &ex.lambda_names {
            names.insert((unit.id, id.0), *sym);
        }
    }
    link::link(&mut model, &shapes, pendings);
    let lowered = link::lower(&model);
    let extract_time = t.elapsed();
    if std::env::var_os("SIGNAL_SOLVE_PRIMS").is_some() {
        for key in &model.keys_by_id {
            if let VarKey::Prim(s) = key {
                let sym = elle::value::SymbolId(*s);
                eprintln!(
                    "prim {} = {:?}",
                    rt.symbols().name(sym).unwrap_or("?"),
                    meta.get(&sym)
                );
            }
        }
    }

    let t = Instant::now();
    let solution = fixpoint::solve(&model, &lowered);
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
