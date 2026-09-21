// audited: 2026-09-20
//! Rust host demo — embeds Elle as a scripting engine, walking every step of
//! the lifecycle a host owes.
//!
//! docs/embedding.md
//! docs/impl/region/rules.md
//!
//! Shows the complete lifecycle:
//!   1. Build the runtime (VM, symbol table, compile context, heap, stdlib)
//!   2. Register a custom host primitive
//!   3. Compile + execute Elle code
//!   4. Extract the result
//!   5. Give the result's owning reference back
//!   6. Tear down, and read the census the sweep answers

use elle::primitives::def::{PrimitiveDef, RegionEffect};
use elle::runtime::Runtime;
use elle::signals::Signal;
use elle::value::fiber::SignalBits;
use elle::value::types::Arity;
use elle::{compile_file, Value};

// ── Custom primitive ────────────────────────────────────────────────

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

// ── Main ────────────────────────────────────────────────────────────

fn main() {
    // The runtime registers primitives, loads the stdlib, and points the VM at
    // this instance's own symbol table and compile context.
    let mut rt = Runtime::new();

    // The custom primitive goes into this instance's compile context. The
    // binding's region is rooted through the instance's own heap; the compile
    // context and heap are taken as disjoint borrows.
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

    // Compile and execute — the VM, symbol table and compile context are
    // threaded explicitly, because there is no shared compile state. The three
    // borrows are scoped so the runtime is free again below.
    let source =
        std::fs::read_to_string("demos/embedding/hello.lisp").expect("could not read hello.lisp");
    let result = {
        let (vm, symbols, cctx) = rt.parts();
        let compiled =
            compile_file(&source, symbols, cctx, "hello.lisp").expect("compilation failed");
        vm.execute_scheduled(&compiled.bytecode, cctx)
            .expect("execution failed")
    };

    println!("Result: {}", result);

    // Nothing reads the result again, so its owning reference goes back here
    // (docs/impl/region/rules.md).
    elle::value::arena::release_program_value(rt.heap(), result);

    // The sweep runs here rather than in `rt`'s Drop, so the census it answers
    // can be read. Zero is the contract, and this demo is held to it like any
    // other host.
    let report = rt.teardown();
    println!("Regions after teardown: {}", report.live_regions);
    assert_eq!(
        report.live_regions, 0,
        "{} regions survived: this host kept a reference the run handed it",
        report.live_regions,
    );
}
