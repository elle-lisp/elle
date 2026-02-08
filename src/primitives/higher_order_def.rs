use crate::symbol::SymbolTable;
use crate::vm::VM;

/// Define map, filter, and fold as Lisp functions that support closures
pub fn define_higher_order_functions(vm: &mut VM, symbols: &mut SymbolTable) {
    use crate::read_str;

    // Define map: (lambda (f lst) (if (nil? lst) nil (cons (f (first lst)) (map f (rest lst)))))
    let map_code = r#"
        (define map (lambda (f lst)
          (if (nil? lst)
            nil
            (cons (f (first lst)) (map f (rest lst))))))
    "#;

    // Define filter
    let filter_code = r#"
        (define filter (lambda (p lst)
          (if (nil? lst)
            nil
            (if (p (first lst))
              (cons (first lst) (filter p (rest lst)))
              (filter p (rest lst))))))
    "#;

    // Define fold
    let fold_code = r#"
        (define fold (lambda (f init lst)
          (if (nil? lst)
            init
            (fold f (f init (first lst)) (rest lst)))))
    "#;

    // Execute each definition
    for code in &[map_code, filter_code, fold_code] {
        match read_str(code, symbols) {
            Ok(value) => {
                match crate::compiler::value_to_expr(&value, symbols) {
                    Ok(expr) => {
                        // Compile and evaluate
                        let bytecode = crate::compile(&expr);
                        if let Err(e) = vm.execute(&bytecode) {
                            eprintln!(
                                "Warning: Failed to execute higher-order function definition: {}",
                                e
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: Failed to compile higher-order function definition: {}",
                            e
                        );
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to parse higher-order function definition: {}",
                    e
                );
            }
        }
    }
}
