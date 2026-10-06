// audited: 2026-10-05
//! A spike: solve signal inference across files with z3, and compare the answer with the analyzer's.
//!
//! docs/signals/inference.md
//! docs/modules.md
//!
//! Usage: `signal_solve [--all] [--no-follow] [--determinism] [--expect] [--dump FILE] FILE...`
//!
//! The binary analyzes each FILE and, unless `--no-follow`, every `.lisp`
//! file that its literal imports name. Each file's HIR becomes Datalog facts.
//! z3 computes the least model over all of them at once, and a worklist in
//! Rust computes it again as a check. `--expect` checks the `# expect` lines
//! in the analyzed files against the model.

mod datalog;
mod extract;
mod fixpoint;
mod link;
mod model;
mod report;

use extract::{literal_imports, Extractor, Unit};
use model::{Model, VarKey};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

fn main() {
    let mut all = false;
    let mut follow = true;
    let mut determinism = false;
    let mut expect = false;
    let mut dump: Option<String> = None;
    let mut files = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--all" => all = true,
            "--no-follow" => follow = false,
            "--determinism" => determinism = true,
            "--expect" => expect = true,
            "--dump" => dump = args.next(),
            _ => files.push(a),
        }
    }
    if files.is_empty() {
        eprintln!("usage: signal_solve [--all] [--no-follow] [--determinism] [--expect] [--dump FILE] FILE...");
        std::process::exit(2);
    }

    let mut rt = elle::runtime::Runtime::new();
    let t = Instant::now();
    let mut units: Vec<Unit> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();
    let mut queue: VecDeque<String> = files.iter().map(|f| extract::canonical(f)).collect();
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
        literal_imports(&analysis.hir, &analysis.arena, &mut imports);
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

    let lowered = link::lower(&model);
    let program = datalog::program(&model, &lowered, false);
    if let Some(path) = &dump {
        std::fs::write(path, &program).expect("write the program");
    }
    let t = Instant::now();
    let solution = match datalog::solve(&program) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let solve_time = t.elapsed();
    let t = Instant::now();
    let native = fixpoint::solve(&model, &lowered);
    let native_time = t.elapsed();
    let native_agrees = native.bits == solution.bits && native.dep == solution.dep;

    let deterministic = if determinism {
        let again =
            datalog::solve(&datalog::program(&model, &lowered, true)).expect("second solve");
        Some(again == solution)
    } else {
        None
    };

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
            program_bytes: program.len(),
            deterministic,
            native_time,
            native_agrees,
            expect,
        },
    );
    if failures > 0 {
        eprintln!("{failures} expectation(s) failed");
        std::process::exit(1);
    }
}
