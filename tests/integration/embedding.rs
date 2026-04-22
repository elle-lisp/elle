use elle::context::{set_symbol_table, set_vm_context};
use elle::pipeline::register_repl_binding;
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::SignalBits;
use elle::value::types::Arity;
use elle::{compile_file, eval_all, init_stdlib, register_primitives, SymbolTable, Value, VM};

// ── Custom primitive registration ───────────────────────────────────

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

#[test]
fn test_custom_primitive_registration() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    set_vm_context(&mut vm as *mut VM);
    set_symbol_table(&mut symbols as *mut SymbolTable);
    init_stdlib(&mut vm, &mut symbols);

    // Register the custom primitive
    let sym_id = symbols.intern("host/add-ten");
    let native = Value::native_fn(&HOST_ADD_TEN);
    register_repl_binding(sym_id, native, Signal::silent(), Some(Arity::Exact(1)));

    let result = eval_all("(host/add-ten 32)", &mut symbols, &mut vm, "<test>").unwrap();
    assert_eq!(result.as_int().unwrap(), 42);

    set_vm_context(std::ptr::null_mut());
}

// ── Scheduled execution with I/O ────────────────────────────────────

#[test]
fn test_scheduled_execution() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    set_vm_context(&mut vm as *mut VM);
    set_symbol_table(&mut symbols as *mut SymbolTable);
    init_stdlib(&mut vm, &mut symbols);

    let result = compile_file(
        r#"(let [p (port/open "/dev/null" :write)]
             (port/write p "hello")
             (port/close p)
             :ok)"#,
        &mut symbols,
        "<test>",
    )
    .unwrap();
    let value = vm.execute_scheduled(&result.bytecode, &symbols).unwrap();
    assert!(value.is_keyword());

    set_vm_context(std::ptr::null_mut());
}

// ── Value round-trip ────────────────────────────────────────────────

#[test]
fn test_value_round_trip() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    set_vm_context(&mut vm as *mut VM);
    set_symbol_table(&mut symbols as *mut SymbolTable);
    init_stdlib(&mut vm, &mut symbols);

    // Register a primitive that returns its argument unchanged
    fn identity_prim(args: &[Value]) -> (SignalBits, Value) {
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
        aliases: &[],
    };
    let sym_id = symbols.intern("host/identity");
    let native = Value::native_fn(&IDENTITY);
    register_repl_binding(sym_id, native, Signal::silent(), Some(Arity::Exact(1)));

    // Int round-trip
    let result = eval_all("(host/identity 42)", &mut symbols, &mut vm, "<test>").unwrap();
    assert_eq!(result.as_int().unwrap(), 42);

    // String round-trip
    let result =
        eval_all("(host/identity \"hello\")", &mut symbols, &mut vm, "<test>").unwrap();
    result.with_string(|s| assert_eq!(s, "hello")).unwrap();

    // Bool round-trip
    let result = eval_all("(host/identity true)", &mut symbols, &mut vm, "<test>").unwrap();
    assert!(result.is_truthy());

    // Nil round-trip
    let result = eval_all("(host/identity nil)", &mut symbols, &mut vm, "<test>").unwrap();
    assert!(result.is_nil());

    set_vm_context(std::ptr::null_mut());
}

// ── Step-based execution ────────────────────────────────────────────

#[test]
fn test_step_based_execution() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    set_vm_context(&mut vm as *mut VM);
    set_symbol_table(&mut symbols as *mut SymbolTable);
    init_stdlib(&mut vm, &mut symbols);

    // Use Elle code to create a scheduler, spawn a fiber, step until done
    let code = r#"
        (let [sched (make-async-scheduler)
              f (fiber/new (fn [] (+ 100 200 300)) |:yield|)]
          ((get sched :spawn) f)
          (def @status :pending)
          (while (= status :pending)
            (assign status ((get sched :step) 0)))
          [status (fiber/value f)])
    "#;

    let result = eval_all(code, &mut symbols, &mut vm, "<test>").unwrap();
    // Result should be [:done 600]
    let arr = result.as_array().unwrap();
    assert!(arr[0].is_keyword());
    assert_eq!(arr[1].as_int().unwrap(), 600);

    set_vm_context(std::ptr::null_mut());
}
