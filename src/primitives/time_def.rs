use crate::pipeline::eval;
use crate::symbol::SymbolTable;
use crate::vm::VM;

/// Define time utility functions in Elle
pub fn define_time_functions(vm: &mut VM, symbols: &mut SymbolTable) {
    let stopwatch_code = r#"
        (define time/stopwatch (fn ()
          (coro/new (fn ()
            (let ((start (clock/monotonic)))
              (while #t
                (yield (- (clock/monotonic) start))))))))
    "#;

    let elapsed_code = r#"
        (define time/elapsed (fn (thunk)
          (let ((start (clock/monotonic)))
            (let ((result (thunk)))
              (list result (- (clock/monotonic) start))))))
    "#;

    for code in &[stopwatch_code, elapsed_code] {
        if let Err(e) = eval(code, symbols, vm) {
            eprintln!("Warning: Failed to define time function: {}", e);
        }
    }
}
