// audited: 2026-10-06
// The embedding surface from a host's side: register a primitive, run source,
// read the value back, step the scheduler.
//
// docs/embedding.md

use elle::primitives::def::{PrimitiveDef, RegionEffect};
use elle::runtime::Runtime;
use elle::signals::Signal;
use elle::value::fiber::SignalBits;
use elle::value::types::Arity;
use elle::{compile_file, eval_all, Value};

// Every test drives one `Runtime` (elle::runtime), the per-instance owner of the
// heap, VM, symbol table, and per-instance `CompileCtx`. `rt.parts()` hands out
// the disjoint borrows the pipeline threads; the host registers a custom
// primitive binding into *this* instance's `CompileCtx` (no shared compile
// cache), and `Runtime` has already pointed the VM at its own symbol table and
// `CompileCtx`.

// ── Custom primitive registration ───────────────────────────────────

fn host_add_ten(
    _ctx: &mut elle::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let n = args[0].as_int().unwrap();
    (SignalBits::EMPTY, Value::int(n + 10))
}

static HOST_ADD_TEN: PrimitiveDef = PrimitiveDef {
    name: "host/add-ten",
    func: host_add_ten,
    signal: Signal::silent(),
    arity: Arity::Exact(1),
    doc: "Add 10 to an integer",
    params: &["n"],
    category: "host",
    example: "(host/add-ten 32)",
    effect: RegionEffect::Immediate,
    ..PrimitiveDef::DEFAULT
};

#[test]
fn test_custom_primitive_registration() {
    let mut rt = Runtime::new();

    // Register the custom primitive into this instance's compile context.
    let sym_id = rt.symbols().intern("host/add-ten");
    let native = Value::native_fn(&HOST_ADD_TEN);
    let (cctx, heap) = rt.compile_and_heap();
    cctx.register_host_binding(
        heap,
        sym_id,
        native,
        elle::value::arena::RootRef::Take,
        Signal::silent(),
        Some(Arity::Exact(1)),
    );

    let (vm, symbols, cctx) = rt.parts();
    let result = eval_all("(host/add-ten 32)", symbols, vm, cctx, "<test>").unwrap();
    assert_eq!(result.as_int().unwrap(), 42);
}

/// A host binding reaches every compile in the instance, the file a program
/// imports included: that file compiles on the instance's context when the
/// import runs, not on the program's.
#[test]
fn a_host_binding_reaches_a_file_a_program_imports() {
    let mut rt = Runtime::new();
    let sym_id = rt.symbols().intern("host/add-ten");
    let native = Value::native_fn(&HOST_ADD_TEN);
    let (cctx, heap) = rt.compile_and_heap();
    cctx.register_host_binding(
        heap,
        sym_id,
        native,
        elle::value::arena::RootRef::Take,
        Signal::silent(),
        Some(Arity::Exact(1)),
    );

    let dir = tempfile::tempdir().expect("scratch dir");
    let module = dir.path().join("m.lisp");
    std::fs::write(&module, "(host/add-ten 32)\n").expect("write m.lisp");
    let program = format!("(import-file \"{}\")", module.display());
    let (vm, symbols, cctx) = rt.parts();
    let result = eval_all(&program, symbols, vm, cctx, "<test>").expect("the import runs");
    assert_eq!(result.as_int(), Some(42));
}

/// A REPL binding reaches a later REPL line and no other compile. The
/// counter-factual registered it with the instance, so `compile_file` resolved
/// it too.
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
    let value = vm
        .execute_scheduled(&line.bytecode, cctx)
        .expect("the line runs");
    assert_eq!(value.as_int(), Some(6));

    let err = compile_file("(+ repl-x 1)", symbols, cctx, "<file>")
        .expect_err("a file must not resolve the REPL binding");
    assert!(
        err.contains("undefined variable: repl-x"),
        "the file fails on the unbound name: {err}"
    );
}

// ── Scheduled execution with I/O ────────────────────────────────────

#[test]
fn test_scheduled_execution() {
    let mut rt = Runtime::new();
    let (vm, symbols, cctx) = rt.parts();

    let result = compile_file(
        r#"(let [p (port/open "/dev/null" :write)]
             (port/write p "hello")
             (port/close p)
             :ok)"#,
        symbols,
        cctx,
        "<test>",
    )
    .unwrap();
    let value = vm.execute_scheduled(&result.bytecode, cctx).unwrap();
    assert!(value.is_keyword());
}

// ── Value round-trip ────────────────────────────────────────────────

#[test]
fn test_value_round_trip() {
    let mut rt = Runtime::new();

    // Register a primitive that returns its argument unchanged
    fn identity_prim(
        _ctx: &mut elle::primitives::ctx::NativeCtx<'_>,
        args: &[Value],
    ) -> (SignalBits, Value) {
        (SignalBits::EMPTY, args[0])
    }
    static IDENTITY: PrimitiveDef = PrimitiveDef {
        name: "host/identity",
        func: identity_prim,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Return argument unchanged",
        params: &["x"],
        category: "host",
        example: "(host/identity 1)",
        ..PrimitiveDef::DEFAULT
    };
    let sym_id = rt.symbols().intern("host/identity");
    let native = Value::native_fn(&IDENTITY);
    let (cctx, heap) = rt.compile_and_heap();
    cctx.register_host_binding(
        heap,
        sym_id,
        native,
        elle::value::arena::RootRef::Take,
        Signal::silent(),
        Some(Arity::Exact(1)),
    );

    let (vm, symbols, cctx) = rt.parts();
    // Int round-trip
    let result = eval_all("(host/identity 42)", symbols, vm, cctx, "<test>").unwrap();
    assert_eq!(result.as_int().unwrap(), 42);

    // String round-trip
    let result = eval_all("(host/identity \"hello\")", symbols, vm, cctx, "<test>").unwrap();
    result.with_string(|s| assert_eq!(s, "hello")).unwrap();

    // Bool round-trip
    let result = eval_all("(host/identity true)", symbols, vm, cctx, "<test>").unwrap();
    assert!(result.is_truthy());

    // Nil round-trip
    let result = eval_all("(host/identity nil)", symbols, vm, cctx, "<test>").unwrap();
    assert!(result.is_nil());
}

// ── Step-based execution ────────────────────────────────────────────

#[test]
fn test_step_based_execution() {
    let mut rt = Runtime::new();
    let (vm, symbols, cctx) = rt.parts();

    // Use Elle code to create a scheduler, spawn a fiber, step until done
    let code = r#"
        (let [sched (make-async-scheduler)
              f (fiber/new (fn [] (+ 100 200 300)) |:yield|)]
          ((get sched :spawn) f)
          (def @status :pending)
          (while (= status :pending)
            (assign status ((get sched :step) :timeout 0)))
          [status (fiber/value f)])
    "#;

    let result = eval_all(code, symbols, vm, cctx, "<test>").unwrap();
    // Result should be [:done 600]
    let arr = result.as_array().unwrap();
    assert!(arr[0].is_keyword());
    assert_eq!(arr[1].as_int().unwrap(), 600);
}
