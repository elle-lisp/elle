// Debug test for printing raw bytecode
use elle::pipeline::compile;
use elle::symbol::SymbolTable;

fn setup() -> (SymbolTable, elle::vm::VM) {
    let mut vm = elle::vm::VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = elle::primitives::register_primitives(&mut vm, &mut symbols);
    (symbols, vm)
}

#[test]
fn test_print_raw_bytecode() {
    let (mut symbols, mut _vm) = setup();

    let code = r#"(begin
        (def process (fn (acc x) (begin (var doubled (* x 2)) (+ acc doubled))))
        (def my-fold (fn (f init lst)
            (if (nil? lst)
                init
                (my-fold f (f init (first lst)) (rest lst)))))
        (my-fold process 0 (list 1 2)))"#;

    let result = compile(code, &mut symbols).expect("compile failed");

    println!("=== RAW BYTES ===");
    for (i, byte) in result.bytecode.instructions.iter().enumerate() {
        println!("  [{}] = 0x{:02x} ({})", i, byte, byte);
    }

    println!("\n=== CONSTANTS ({}) ===", result.bytecode.constants.len());
    for (i, c) in result.bytecode.constants.iter().enumerate() {
        if let Some(closure) = c.as_closure() {
            println!("  [{}] = Closure:", i);
            println!("    bytecode len: {}", closure.bytecode.len());
            println!("    constants len: {}", closure.constants.len());
            println!(
                "    raw bytes: {:?}",
                &closure.bytecode[..std::cmp::min(20, closure.bytecode.len())]
            );
        } else {
            println!("  [{}] = {:?}", i, c);
        }
    }
}
