use crate::pipeline::eval;
use crate::symbol::SymbolTable;
use crate::vm::VM;

/// Define map, filter, and fold as Lisp functions that support closures
pub fn define_higher_order_functions(vm: &mut VM, symbols: &mut SymbolTable) {
    // Define map: polymorphic across all sequence types
    // Indexed types and strings are checked before pair/empty to avoid
    // calling empty? on types that don't support it (bytes, blob).
    let map_code = r#"
        (def map (fn (f coll)
          (cond
            ((or (array? coll) (tuple? coll) (bytes? coll) (blob? coll))
             (letrec ((loop (fn (i acc)
                              (if (>= i (length coll))
                                (reverse acc)
                                (loop (+ i 1) (cons (f (get coll i)) acc))))))
               (loop 0 ())))
            ((or (string? coll) (buffer? coll))
             (letrec ((loop (fn (i acc)
                              (if (>= i (length coll))
                                (reverse acc)
                                (loop (+ i 1) (cons (f (string/char-at coll i)) acc))))))
               (loop 0 ())))
            ((or (pair? coll) (empty? coll))
             (if (empty? coll)
               ()
               (cons (f (first coll)) (map f (rest coll)))))
            (true (error :type-error "map: not a sequence")))))
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
