//! Rust host demo — embeds Elle as a scripting engine.
//!
//! Shows the complete lifecycle:
//!   1. Create VM + SymbolTable
//!   2. Register primitives + stdlib
//!   3. Register a custom host primitive
//!   4. Compile + execute Elle code
//!   5. Extract result
//!   6. Cleanup

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
    // 1–3. Create runtime: registers primitives, loads the stdlib, and points the
    //      VM at this instance's own symbol table and compile context.
    let mut rt = Runtime::new();

    // 4. Register custom primitive into this instance's compile context. The
    //    binding's region is rooted through the instance's own heap; the compile
    //    context and heap are taken as disjoint borrows.
    let sym_id = rt.symbols().intern("host/add-ten");
    let native = Value::native_fn(&HOST_ADD_TEN);
    let (cctx, heap) = rt.compile_and_heap();
    cctx.register_repl_binding(
        heap,
        sym_id,
        native,
        Signal::silent(),
        Some(Arity::Exact(1)),
    );

    // 5. Compile + execute — thread the VM, symbol table, and compile context
    //    explicitly (there is no shared compile state).
    let source =
        std::fs::read_to_string("demos/embedding/hello.lisp").expect("could not read hello.lisp");
    let (vm, symbols, cctx) = rt.parts();
    let compiled = compile_file(&source, symbols, cctx, "hello.lisp").expect("compilation failed");
    let result = vm
        .execute_scheduled(&compiled.bytecode, cctx)
        .expect("execution failed");

    // 6. Extract result
    println!("Result: {}", result);

    // 7. Cleanup — `rt` drops here, running the RC teardown sweep; the VM's
    //    symbol-table and compile-context pointers drop with the instance.
}
