use crate::pipeline::eval_new;
use crate::symbol::SymbolTable;
use crate::vm::VM;

/// Define map, filter, and fold as Lisp functions that support closures
pub fn define_higher_order_functions(vm: &mut VM, symbols: &mut SymbolTable) {
    // Define map: (lambda (f lst) (if (empty? lst) () (cons (f (first lst)) (map f (rest lst)))))
    let map_code = r#"
        (define map (lambda (f lst)
          (if (empty? lst)
            ()
            (cons (f (first lst)) (map f (rest lst))))))
    "#;

    // Define filter
    let filter_code = r#"
        (define filter (lambda (p lst)
          (if (empty? lst)
            ()
            (if (p (first lst))
              (cons (first lst) (filter p (rest lst)))
              (filter p (rest lst))))))
    "#;

    // Define fold
    let fold_code = r#"
        (define fold (lambda (f init lst)
          (if (empty? lst)
            init
            (fold f (f init (first lst)) (rest lst)))))
    "#;

    // Execute each definition using the new pipeline
    for code in &[map_code, filter_code, fold_code] {
        if let Err(e) = eval_new(code, symbols, vm) {
            eprintln!("Warning: Failed to define higher-order function: {}", e);
        }
    }
}
