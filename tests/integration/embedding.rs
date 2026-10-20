// audited: 2026-10-20
// The embedding surface from a host's side: register a primitive, run source,
// read the value back, step the scheduler, and reach a worker.
//
// docs/embedding.md
// docs/threads.md

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
    cctx.register_repl_binding(
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

// ── A registered primitive reaches the instance's workers ───────────

/// What `src` answers on an instance with `host/add-ten` registered through
/// `Runtime::register_primitive`, or the error it raised.
fn on_registered(src: &str) -> Result<i64, String> {
    let mut rt = Runtime::new();
    rt.register_primitive(&HOST_ADD_TEN);
    let (vm, symbols, cctx) = rt.parts();
    eval_all(src, symbols, vm, cctx, "<test>").map(|v| v.as_int().expect("an integer"))
}

#[test]
fn a_registered_primitive_resolves_in_compiled_code_and_in_eval() {
    assert_eq!(on_registered("(host/add-ten 32)"), Ok(42));
    assert_eq!(on_registered("(eval '(host/add-ten 32))"), Ok(42));
}

#[test]
fn code_a_worker_compiles_names_the_primitives_its_instance_registered() {
    // The counter-factual: a worker built with the core primitives alone runs
    // a closure that calls `host/add-ten`, because the call rides in the
    // closure's constants, and fails on an `eval` that names it. The test
    // runner compiles each file in a worker, so its files never saw the rig's
    // primitives.
    assert_eq!(
        on_registered("(sys/join (sys/spawn (fn [] (host/add-ten 32))))"),
        Ok(42),
        "a compiled call crosses in the closure"
    );
    assert_eq!(
        on_registered("(sys/join (sys/spawn (fn [] (eval '(host/add-ten 32)))))"),
        Ok(42),
        "the heavy worker resolves the name"
    );
    assert_eq!(
        on_registered("(sys/join (sys/spawn-vm (fn [] (eval '(host/add-ten 32)))))"),
        Ok(42),
        "and so does the light worker"
    );
}

#[test]
fn a_worker_passes_the_primitives_on_to_the_workers_it_spawns() {
    // The counter-factual: a worker that registers what it inherited but
    // records none of it hands its own workers the core primitives alone.
    assert_eq!(
        on_registered(
            "(sys/join (sys/spawn (fn [] \
               (ev/run (fn [] (sys/join (sys/spawn (fn [] (eval '(host/add-ten 32))))))))))"
        ),
        Ok(42)
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
    let value = vm.execute_scheduled(&result, cctx).unwrap();
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
    cctx.register_repl_binding(
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
