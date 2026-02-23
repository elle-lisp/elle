use crate::pipeline::eval;
use crate::symbol::SymbolTable;
use crate::vm::VM;

/// Define map, filter, and fold as Lisp functions that support closures
pub fn define_higher_order_functions(vm: &mut VM, symbols: &mut SymbolTable) {
    // Define map: (fn (f lst) (if (empty? lst) () (cons (f (first lst)) (map f (rest lst)))))
    let map_code = r#"
        (def map (fn (f lst)
          (if (empty? lst)
            ()
            (cons (f (first lst)) (map f (rest lst))))))
    "#;

    // Define filter
    let filter_code = r#"
        (def filter (fn (p lst)
          (if (empty? lst)
            ()
            (if (p (first lst))
              (cons (first lst) (filter p (rest lst)))
              (filter p (rest lst))))))
    "#;

    // Define fold
    let fold_code = r#"
        (def fold (fn (f init lst)
          (if (empty? lst)
            init
            (fold f (f init (first lst)) (rest lst)))))
    "#;

    // Execute each definition using the new pipeline
    for code in &[map_code, filter_code, fold_code] {
        if let Err(e) = eval(code, symbols, vm) {
            eprintln!("Warning: Failed to define higher-order function: {}", e);
        }
    }
}
