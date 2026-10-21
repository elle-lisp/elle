// audited: 2026-10-21
// Which compiles a registration reaches: a file a program imports, the next
// load of a kept file, and a REPL binding's REPL lines alone.
//
// docs/embedding.md
// docs/pipeline.md

use super::*;

/// A host binding reaches every compile in the instance, the file a program
/// imports included: that file compiles on the instance's context when the
/// import runs, not on the program's.
#[test]
fn a_host_binding_reaches_a_file_a_program_imports() {
    let mut rt = Runtime::new();
    bind_host(
        &mut rt,
        "host/add-ten",
        Value::native_fn(&HOST_ADD_TEN),
        Some(Arity::Exact(1)),
    );
    let dir = tempfile::tempdir().expect("scratch dir");
    let module = dir.path().join("m.lisp");
    std::fs::write(&module, "(host/add-ten 32)\n").expect("write m.lisp");
    assert_eq!(load_int(&mut rt, &module), Some(42));
}

/// Load `module` through `import-file` from a program run on `rt`, and answer
/// the module's value as an integer.
fn load_int(rt: &mut Runtime, module: &std::path::Path) -> Option<i64> {
    let program = format!("(import-file \"{}\")", module.display());
    let (vm, symbols, cctx) = rt.parts();
    eval_all(&program, symbols, vm, cctx, "<test>")
        .expect("the import runs")
        .as_int()
}

/// The instance keeps a file whose compile resolved `host/v` to 1. A host
/// binding registered before the next load reaches that load. The
/// counter-factual would run the kept code, which would still answer 1.
#[test]
fn a_host_binding_reaches_the_next_load_of_a_loaded_file() {
    let mut rt = Runtime::new();
    bind_host(&mut rt, "host/v", Value::int(1), None);
    let dir = tempfile::tempdir().expect("scratch dir");
    let module = dir.path().join("m.lisp");
    std::fs::write(&module, "host/v\n").expect("write m.lisp");
    assert_eq!(load_int(&mut rt, &module), Some(1));
    bind_host(&mut rt, "host/v", Value::int(2), None);
    assert_eq!(load_int(&mut rt, &module), Some(2));
}

fn host_two(
    _ctx: &mut elle::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SignalBits::EMPTY, Value::int(2))
}

/// A primitive that takes over the name `host/v`.
static HOST_V: PrimitiveDef = PrimitiveDef {
    name: "host/v",
    func: host_two,
    signal: Signal::silent(),
    arity: Arity::Exact(0),
    doc: "Answer 2.",
    params: &[],
    category: "host",
    example: "(host/v)",
    effect: RegionEffect::Immediate,
    ..PrimitiveDef::DEFAULT
};

/// A primitive the host registers between two loads of one file reaches the
/// second load, as a binding does. The counter-factual would run the kept code,
/// which would read `host/v` as the integer the first compile saw.
#[test]
fn a_primitive_reaches_the_next_load_of_a_loaded_file() {
    let mut rt = Runtime::new();
    bind_host(&mut rt, "host/v", Value::int(1), None);
    let dir = tempfile::tempdir().expect("scratch dir");
    let module = dir.path().join("m.lisp");
    std::fs::write(&module, "(if (integer? host/v) host/v (host/v))\n").expect("write m.lisp");
    assert_eq!(load_int(&mut rt, &module), Some(1));
    rt.register_primitive(&HOST_V);
    assert_eq!(load_int(&mut rt, &module), Some(2));
}

/// A REPL binding reaches a later REPL line and no other compile. The
/// counter-factual would register it with the instance, so `compile_file` would
/// resolve it too.
#[test]
fn a_repl_binding_reaches_a_repl_line_and_no_file() {
    let mut rt = Runtime::new();
    let sym_id = rt.symbols().intern("repl-x");
    let (cctx, heap) = rt.compile_and_heap();
    cctx.register_repl_binding(
        heap,
        sym_id,
        Value::int(5),
        elle::value::arena::RootRef::Take,
        Signal::silent(),
        None,
    );

    let (vm, symbols, cctx) = rt.parts();
    let (line, _) = elle::pipeline::compile_file_repl("(+ repl-x 1)", symbols, cctx, "<repl>")
        .expect("a REPL line resolves the REPL binding");
    let value = vm.execute_scheduled(&line, cctx).expect("the line runs");
    assert_eq!(value.as_int(), Some(6));

    let err = compile_file("(+ repl-x 1)", symbols, cctx, "<file>")
        .expect_err("a file must not resolve the REPL binding");
    assert!(
        err.contains("undefined variable: repl-x"),
        "the file fails on the unbound name: {err}"
    );
}
