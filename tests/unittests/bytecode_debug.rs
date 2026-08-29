// Debug test for printing raw bytecode
use elle::symbol::SymbolTable;

// Local `compile` shim preserving the pre-CompileCtx arity. The test source
// uses stdlib (`nil?`), so load the stdlib: it must be in `cctx.meta` for name
// resolution, while its export closures live on a throwaway VM (this test only
// inspects the compiled bytecode, it never executes).
fn compile(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
) -> Result<elle::CompileResult, String> {
    let mut vm = elle::vm::VM::new();
    let _ = elle::register_primitives(&mut vm, symbols);
    let mut cctx = elle::pipeline::CompileCtx::new();
    vm.set_symbols(symbols as *mut SymbolTable);
    elle::init_stdlib(&mut vm, symbols, &mut cctx, &elle::compiler::stdlib_cache::StdlibCache::Off);
    elle::pipeline::compile(source, symbols, &mut cctx, source_name)
}

fn setup() -> (SymbolTable, elle::vm::VM) {
    let mut vm = elle::vm::VM::new();
    let mut symbols = SymbolTable::new();
    let _signals = elle::primitives::register_primitives(&mut vm, &mut symbols);
    (symbols, vm)
}

#[test]
fn test_print_raw_bytecode() {
    let (mut symbols, mut _vm) = setup();

    let code = r#"(begin
        (def process (fn (acc x) (numeric!) (begin (var doubled (%mul x 2)) (%add acc doubled))))
        (def my-fold (fn (f init lst)
            (if (nil? lst)
                init
                (my-fold f (f init (first lst)) (rest lst)))))
        (my-fold process 0 (list 1 2)))"#;

    let result = compile(code, &mut symbols, "<test>").expect("compile failed");

    println!("=== RAW BYTES ===");
    for (i, byte) in result.bytecode.instructions.iter().enumerate() {
        println!("  [{}] = 0x{:02x} ({})", i, byte, byte);
    }

    println!("\n=== CONSTANTS ({}) ===", result.bytecode.constants.len());
    for (i, c) in result.bytecode.constants.iter().enumerate() {
        if let Some(closure) = c.as_closure() {
            println!("  [{}] = Closure:", i);
            println!("    bytecode len: {}", closure.template.bytecode.len());
            println!("    constants len: {}", closure.template.constants.len());
            println!(
                "    raw bytes: {:?}",
                &closure.template.bytecode[..std::cmp::min(20, closure.template.bytecode.len())]
            );
        } else {
            println!("  [{}] = {:?}", i, c);
        }
    }
}
