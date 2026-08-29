//! The `--trace=scrub` page-blanking diagnostic must not disturb a correct
//! program.
//!
//! Scrub zeroes the spans a dying region wrote before its page goes back to the
//! cache, so a read through a pointer that outlived its region lands on an
//! all-zero slot and detonates at the deref instead of returning plausible
//! bytes (docs/impl/region/model.md § "Page recycling").
//!
//! A diagnostic is only worth its verdict if it changes nothing else, and this
//! one writes memory. So the pin is the round trip: run allocation-heavy work
//! with scrub armed and require the same answers as without it. Every shape
//! below churns regions — per-iteration aggregates, closures over captured
//! data, nested collections, string building — so pages are released and
//! re-claimed continuously, and a scrub aimed at the wrong page, the wrong
//! length, or across the page header reaches something that matters quickly.
//! A run that comes back with the right answers is what lets a scrub failure
//! elsewhere be read as a use-after-free rather than as scrub damage.
//!
//! This file is its OWN test binary because `config::init` is process-global:
//! arming scrub inside the shared integration binary would blank pages for
//! every other test in it.

use elle::{init_stdlib, register_primitives, SymbolTable, VM};

/// Run `source` on a fresh VM and return its result rendered as a string.
fn run(source: &str) -> String {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _ = register_primitives(&mut vm, &mut symbols);
    let mut cctx = elle::pipeline::CompileCtx::new();
    vm.set_compile_ctx(&mut cctx as *mut elle::pipeline::CompileCtx);
    vm.set_symbols(&mut symbols as *mut SymbolTable);
    init_stdlib(
        &mut vm,
        &mut symbols,
        &mut cctx,
        &elle::compiler::stdlib_cache::StdlibCache::Off,
    );
    match elle::eval_all(source, &mut symbols, &mut vm, &mut cctx, "<scrub>") {
        Ok(v) => format!("{}", v.display_with(Some(&symbols))),
        Err(e) => format!("error: {e}"),
    }
}

/// Region-churning programs with answers that do not depend on allocation
/// behaviour. Each is ONE expression, so the same text can be evaluated
/// in-process and printed by a subprocess, and each returns a value a
/// corrupted page would change.
const SHAPES: &[(&str, &str)] = &[
    (
        "sum of per-iteration aggregates",
        "((fn []
            (var acc 0)
            (each i in (range 0 400)
              (let [row {:n i :sq (* i i)}]
                (assign acc (+ acc (get row :sq)))))
            acc))",
    ),
    (
        "list built and re-read per iteration",
        "((fn []
            (var total 0)
            (each i in (range 0 300)
              (let [xs (map (fn [x] (* x 2)) [i (+ i 1) (+ i 2)])]
                (assign total (+ total (length xs) (first xs)))))
            total))",
    ),
    (
        "closures over captured data, called after capture",
        "((fn []
            (var total 0)
            (each i in (range 0 300)
              (let [vals [i (* i 3)]
                    peek (fn [] (get vals 1))]
                (assign total (+ total (peek) (get vals 0)))))
            total))",
    ),
    (
        "strings built by repeated concatenation",
        "((fn []
            (var n 0)
            (each i in (range 0 200)
              (let [s (concat \"row-\" (number->string i) \"-end\")]
                (assign n (+ n (length s)))))
            n))",
    ),
    (
        "nested collections read back after construction",
        "((fn []
            (var total 0)
            (each i in (range 0 200)
              (let [m {:rows [[i 1] [i 2]] :tag \"m\"}]
                (assign total (+ total (get (get (get m :rows) 1) 1)))))
            total))",
    ),
];

#[test]
fn scrub_leaves_a_correct_program_correct() {
    // Compute the reference answers in a child process WITHOUT scrub, because
    // `config::init` is one-shot per process: this binary can only ever be
    // armed or unarmed, never both. The reference is the plain `elle` binary
    // this test's crate builds alongside it.
    let exe = env!("CARGO_BIN_EXE_elle");
    let dir = std::env::temp_dir().join(format!("elle-scrub-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");

    let mut expected: Vec<String> = Vec::new();
    for (i, (name, source)) in SHAPES.iter().enumerate() {
        let path = dir.join(format!("shape{i}.lisp"));
        std::fs::write(&path, format!("(println {source})")).expect("write shape");
        let out = std::process::Command::new(exe)
            .arg(&path)
            .output()
            .expect("run the reference binary");
        assert!(
            out.status.success(),
            "reference run of {name} failed: {}",
            String::from_utf8_lossy(&out.stderr),
        );
        expected.push(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    std::fs::remove_dir_all(&dir).ok();

    // Now arm scrub for THIS process and re-run every shape in-process.
    let mut cfg = elle::config::Config::default();
    cfg.trace_keywords.push("scrub".to_string());
    elle::config::init(cfg);

    for ((name, source), want) in SHAPES.iter().zip(expected) {
        let got = run(source);
        assert_eq!(
            got, want,
            "under --trace=scrub, {name} answered {got} instead of {want} — a \
             released page's scrub reached memory a live region still owns, so \
             the PageDirty spans overstate what the dying region wrote",
        );
    }
}
