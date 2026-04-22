//! Rust host demo — embeds Elle as a scripting engine.
//!
//! Shows the complete lifecycle:
//!   1. Create VM + SymbolTable
//!   2. Register primitives + stdlib
//!   3. Register a custom host primitive
//!   4. Compile + execute Elle code
//!   5. Extract result
//!   6. Cleanup

use elle::context::{set_symbol_table, set_vm_context};
use elle::pipeline::register_repl_binding;
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::SignalBits;
use elle::value::types::Arity;
use elle::{compile_file, init_stdlib, register_primitives, SymbolTable, Value, VM};

// ── Custom primitive ────────────────────────────────────────────────

fn host_add_ten(args: &[Value]) -> (SignalBits, Value) {
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
    aliases: &[],
};

// ── Main ────────────────────────────────────────────────────────────

fn main() {
    // 1. Create runtime
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    // 2. Set context (required before stdlib init)
    set_vm_context(&mut vm as *mut VM);
    set_symbol_table(&mut symbols as *mut SymbolTable);

    // 3. Load stdlib
    init_stdlib(&mut vm, &mut symbols);

    // 4. Register custom primitive
    let sym_id = symbols.intern("host/add-ten");
    let native = Value::native_fn(&HOST_ADD_TEN);
    register_repl_binding(sym_id, native, Signal::silent(), Some(Arity::Exact(1)));

    // 5. Compile + execute
    let source =
        std::fs::read_to_string("demos/embedding/hello.lisp").expect("could not read hello.lisp");
    let compiled = compile_file(&source, &mut symbols, "hello.lisp").expect("compilation failed");
    let result = vm
        .execute_scheduled(&compiled.bytecode, &symbols)
        .expect("execution failed");

    // 6. Extract result
    println!("Result: {}", result);

    // 7. Cleanup
    set_vm_context(std::ptr::null_mut());
}
