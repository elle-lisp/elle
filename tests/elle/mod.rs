// Elle script tests — each .lisp file in this directory is a test.
//
// Scripts use assertions.lisp helpers (assert-eq, assert-true, etc.)
// which call (exit 1) on failure. A zero exit code means all
// assertions passed.

use std::process::Command;

fn get_elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

fn run_elle_script(name: &str) {
    let path = format!("tests/elle/{}.lisp", name);
    let output = Command::new(get_elle_binary())
        .arg(&path)
        .output()
        .unwrap_or_else(|e| panic!("Failed to run elle on {}: {}", path, e));

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "{} failed (exit code {:?}):\n--- stdout ---\n{}\n--- stderr ---\n{}",
            path,
            output.status.code(),
            stdout,
            stderr
        );
    }
}

#[test]
fn eval() {
    run_elle_script("eval");
}

#[test]
fn prelude() {
    run_elle_script("prelude");
}

#[test]
fn destructuring() {
    run_elle_script("destructuring");
}

#[test]
fn core() {
    run_elle_script("core");
}

#[test]
fn splice() {
    run_elle_script("splice");
}

#[test]
fn blocks() {
    run_elle_script("blocks");
}

#[test]
fn functional() {
    run_elle_script("functional");
}

#[test]
fn arithmetic() {
    run_elle_script("arithmetic");
}

#[test]
fn determinism() {
    run_elle_script("determinism");
}

#[test]
fn property_eval() {
    run_elle_script("property-eval");
}

#[test]
fn convert() {
    run_elle_script("convert");
}

#[test]
fn sequences() {
    run_elle_script("sequences");
}

#[test]
fn macros() {
    run_elle_script("macros");
}

#[test]
fn strings() {
    run_elle_script("strings");
}

#[test]
fn bugfixes() {
    run_elle_script("bugfixes");
}

#[test]
fn fibers() {
    run_elle_script("fibers");
}

#[test]
fn closures() {
    run_elle_script("closures");
}

#[test]
fn matching() {
    run_elle_script("matching");
}

#[test]
fn sets() {
    run_elle_script("sets");
}

#[test]
fn coroutines() {
    run_elle_script("coroutines");
}

#[test]
fn tailcalls() {
    run_elle_script("tailcalls");
}

#[test]
fn parameters() {
    run_elle_script("parameters");
}

#[test]
fn ports() {
    run_elle_script("ports");
}

#[test]
fn chan() {
    run_elle_script("chan");
}

#[test]
fn io() {
    run_elle_script("io");
}

#[test]
fn advanced() {
    run_elle_script("advanced");
}

#[test]
fn brackets() {
    run_elle_script("brackets");
}

#[test]
fn lexical_scope() {
    run_elle_script("lexical-scope");
}

#[test]
fn primitives() {
    run_elle_script("primitives");
}

#[test]
fn pipeline() {
    run_elle_script("pipeline");
}

#[test]
fn jit_yield() {
    run_elle_script("jit-yield");
}

#[test]
fn buffer() {
    run_elle_script("buffer");
}

#[test]
fn bytes() {
    run_elle_script("bytes");
}

#[test]
fn table_keys() {
    run_elle_script("table-keys");
}

#[test]
fn environment() {
    run_elle_script("environment");
}

#[test]
fn concurrency() {
    run_elle_script("concurrency");
}

#[test]
fn ffi() {
    run_elle_script("ffi");
}

#[test]
fn glob() {
    run_elle_script("glob");
}

#[test]
fn fn_flow() {
    run_elle_script("fn-flow");
}

#[test]
fn fn_graph() {
    run_elle_script("fn-graph");
}

#[test]
fn fn_graph_2() {
    run_elle_script("fn-graph-2");
}

#[test]
fn fn_graph_3() {
    run_elle_script("fn-graph-3");
}

#[test]
fn arena() {
    run_elle_script("arena");
}

#[test]
fn new_pipeline() {
    run_elle_script("new-pipeline");
}

#[test]
fn ordering() {
    run_elle_script("ordering");
}

#[test]
fn slice() {
    run_elle_script("slice");
}

#[test]
fn jit_variadic() {
    run_elle_script("jit-variadic");
}

#[test]
fn regex() {
    run_elle_script("regex");
}

#[test]
fn compliance() {
    run_elle_script("compliance");
}

#[test]
fn net() {
    run_elle_script("net");
}
