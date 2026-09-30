// audited: 2026-09-30
// Under --dump=stats a process reports every reclamation counter, summed over every heap it ran.
//
// docs/impl/region/diagnostics.md
//
// The counter-factual these guard: a report that read the main heap alone
// would still print every line, and every identity would still close. Only a
// count that must arrive from another heap tells the two apart, so the worker
// and `elle test` cases compare a run whose other heap adopts against one
// whose other heap does not.

use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// Every line of the report, in the order diagnostics.md's table gives.
const NAMES: &[&str] = &[
    "region-frees",
    "page-frees",
    "object-frees",
    "one-page-frees",
    "empty-frees",
    "one-object-frees",
    "few-object-frees",
    "many-object-frees",
    "adopts",
    "adopts-into-empty",
    "owned-frees",
    "owned-free-pages",
    "owned-free-objects",
    "owned-one-page-frees",
    "rescues",
    "rescue-survivors",
    "extracts",
    "reparents",
    "owned",
];

/// A function whose push adopts the pushed array into the container, and whose
/// return frees both: one adoption per call, ended by an owned free.
const KEEP_ONE: &str = "(defn keep-one []\n  (let [c (@array)]\n    (push c (array 1 2))\n    nil))\n\
     (defn keep-many [n]\n  (var i 0)\n  (while (< i n)\n    (keep-one)\n    (assign i (+ i 1))))\n";

/// The report's lines out of `stderr`, as (name, value) in printed order.
fn report(stderr: &str) -> Vec<(String, i64)> {
    stderr
        .lines()
        .filter_map(|l| l.strip_prefix("[stats] reclaim "))
        .map(|kv| {
            let (k, v) = kv
                .split_once('=')
                .unwrap_or_else(|| panic!("a report line is name=value, got {kv:?}"));
            let n = v
                .trim()
                .parse()
                .unwrap_or_else(|e| panic!("{k} is not a number ({e}): {v:?}"));
            (k.to_string(), n)
        })
        .collect()
}

fn value(lines: &[(String, i64)], name: &str) -> i64 {
    lines
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| panic!("no {name} line in the report: {lines:?}"))
}

/// Run `elle ARGS` with the program `src` written to a scratch file, and answer
/// its stderr.
fn run(tag: &str, args: &[&str], src: &str) -> String {
    let dir = crate::common::ScratchDir::new(tag);
    let path = dir.join("prog.lisp");
    std::fs::write(&path, src).unwrap();
    let out = Command::new(elle_binary())
        .args(args)
        .arg(&path)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        out.status.success(),
        "elle {args:?} failed; stderr:\n{stderr}"
    );
    stderr
}

#[test]
fn stats_reports_every_counter_in_order_and_the_size_buckets_close() {
    let src = format!("{KEEP_ONE}(keep-many 50)\n");
    let lines = report(&run("reclaim-report", &["--dump=stats"], &src));

    let names: Vec<&str> = lines.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        names, NAMES,
        "the report prints every counter once, in the table's order"
    );
    let frees = value(&lines, "region-frees");
    assert!(frees > 0, "the run freed regions, so the sums are not vacuous");
    let buckets: i64 = ["empty-frees", "one-object-frees", "few-object-frees", "many-object-frees"]
        .iter()
        .map(|k| value(&lines, k))
        .sum();
    assert_eq!(
        buckets, frees,
        "summed over the process, the object buckets still partition region-frees"
    );
    assert!(
        value(&lines, "one-page-frees") <= frees,
        "a one-page free is a region free"
    );
    assert!(
        value(&lines, "adopts") >= 50,
        "each of the 50 calls adopts once, got {lines:?}"
    );
    assert!(
        value(&lines, "owned") >= 0,
        "owned is the adoptions not yet ended, which cannot be negative: {lines:?}"
    );
}

#[test]
fn without_stats_there_is_no_report() {
    let src = format!("{KEEP_ONE}(keep-many 5)\n");
    let stderr = run("reclaim-no-report", &[], &src);
    assert!(
        report(&stderr).is_empty(),
        "a run without --dump=stats prints no reclaim line; stderr:\n{stderr}"
    );
}

#[test]
fn the_report_counts_the_heap_of_a_worker_thread() {
    // The two programs differ only in what the worker does, so the difference
    // in adoptions is the worker heap's alone.
    let idle = format!("{KEEP_ONE}(os/join (os/spawn (fn [] nil)))\n");
    let busy = format!("{KEEP_ONE}(os/join (os/spawn (fn [] (keep-many 200))))\n");
    let idle = value(&report(&run("reclaim-idle", &["--dump=stats"], &idle)), "adopts");
    let busy = value(&report(&run("reclaim-busy", &["--dump=stats"], &busy)), "adopts");
    assert!(
        busy - idle >= 200,
        "the worker's 200 adoptions must reach the process report: idle {idle}, busy {busy}"
    );
}

#[test]
fn elle_test_stats_reports_the_heaps_its_test_code_ran_on() {
    // The runner's own heap adopts nothing for either file (docs/test-gauges.md);
    // each tier run of the adopting form adopts once, on a worker's heap. The
    // runner ends through os/exit, so this also covers the report on that path.
    let dir = crate::common::ScratchDir::new("reclaim-elle-test");
    let run_test = |name: &str, src: &str| -> i64 {
        let path = dir.join(name);
        std::fs::write(&path, src).unwrap();
        let out = Command::new(elle_binary())
            .args(["test", "--dump=stats"])
            .arg(&path)
            .args(["--timeout", "30000", "--db"])
            .arg(dir.join(&format!("{name}.db")))
            .env_remove("RUST_MIN_STACK")
            .output()
            .expect("run elle test");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "a passing form must gate green; stderr:\n{stderr}"
        );
        value(&report(&stderr), "adopts")
    };
    let plain = run_test("plain.lisp", "(assert true \"ok\")\n");
    let adopting = run_test(
        "adopting.lisp",
        "(let [c (@array)]\n  (push c (array 1 2))\n  (length c))\n",
    );
    assert!(
        adopting > plain,
        "the adopting form's worker adoptions must reach the report: plain {plain}, \
         adopting {adopting}"
    );
}
