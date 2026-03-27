//! Smoke test: compile Elle source through LIR → WASM → Wasmtime.

fn eval(source: &str) -> String {
    match elle::wasm::eval_wasm(source, "<test>") {
        Ok(result) => format!("{}", result),
        Err(e) => panic!("eval_wasm('{}') failed: {}", source, e),
    }
}

// --- Phase 0: arithmetic ---

#[test]
fn test_integer_literal() {
    assert_eq!(eval("42"), "42");
}

#[test]
fn test_add_integers() {
    assert_eq!(eval("(+ 1 2)"), "3");
}

#[test]
fn test_nested_arithmetic() {
    assert_eq!(eval("(+ (* 3 4) (- 10 5))"), "17");
}

#[test]
fn test_negative() {
    assert_eq!(eval("(- 0 5)"), "-5");
}

#[test]
fn test_boolean_literals() {
    assert_eq!(eval("true"), "true");
    assert_eq!(eval("false"), "false");
}

#[test]
fn test_nil() {
    assert_eq!(eval("nil"), "nil");
}

// --- Phase 1: control flow ---

#[test]
fn test_if_true() {
    assert_eq!(eval("(if true 1 2)"), "1");
}

#[test]
fn test_if_false() {
    assert_eq!(eval("(if false 1 2)"), "2");
}

#[test]
fn test_if_nil() {
    assert_eq!(eval("(if nil 1 2)"), "2");
}

#[test]
fn test_if_integer_truthy() {
    assert_eq!(eval("(if 0 1 2)"), "1");
}

#[test]
fn test_let_binding() {
    assert_eq!(eval("(let* [[x 10]] (+ x 1))"), "11");
}

#[test]
fn test_let_two_bindings() {
    assert_eq!(eval("(let* [[x 10] [y 20]] (+ x y))"), "30");
}

#[test]
fn test_if_with_comparison() {
    assert_eq!(eval("(if (> 5 3) (+ 1 2) (- 10 5))"), "3");
}

#[test]
fn test_nested_if() {
    assert_eq!(eval("(if true (if false 10 20) 30)"), "20");
}

// --- Phase 1: function calls (primitives) ---

#[test]
fn test_call_length() {
    assert_eq!(eval("(length [1 2 3])"), "3");
}

#[test]
fn test_call_cons() {
    assert_eq!(eval("(cons 1 (list 2 3))"), "(1 2 3)");
}

#[test]
fn test_call_not() {
    // `not` is a UnaryOp intrinsic, but explicit call still works
    assert_eq!(eval("(not false)"), "true");
}

#[test]
fn test_call_type() {
    assert_eq!(eval("(type 42)"), ":integer");
}

#[test]
fn test_call_empty() {
    assert_eq!(eval("(empty? ())"), "true");
    assert_eq!(eval("(empty? (list 1))"), "false");
}

// --- Phase 1: data operations ---

#[test]
fn test_array_literal() {
    assert_eq!(eval("[1 2 3]"), "[1 2 3]");
}

#[test]
fn test_first_rest() {
    assert_eq!(eval("(first (cons 1 2))"), "1");
    assert_eq!(eval("(rest (cons 1 2))"), "2");
}

#[test]
fn test_struct_access() {
    assert_eq!(eval("(get {:x 1 :y 2} :x)"), "1");
}

// --- Phase 1: closures ---

#[test]
fn test_lambda_call() {
    assert_eq!(eval("((fn [x] (+ x 1)) 42)"), "43");
}

#[test]
fn test_let_lambda() {
    assert_eq!(eval("(let* [[f (fn [x] (+ x 1))]] (f 42))"), "43");
}

#[test]
fn test_closure_capture() {
    // Closure captures a value from outer scope
    assert_eq!(eval("(let* [[x 10]] ((fn [y] (+ x y)) 5))"), "15");
}

#[test]
fn test_higher_order() {
    // Pass a primitive as an argument
    assert_eq!(eval("((fn [f x y] (f x y)) + 3 4)"), "7");
}

#[test]
fn test_multi_arg_lambda() {
    assert_eq!(eval("((fn [a b c] (+ a (+ b c))) 1 2 3)"), "6");
}

#[test]
fn test_cond() {
    assert_eq!(
        eval("(defn classify [x]\n  (cond\n    ((< x 0) :negative)\n    ((= x 0) :zero)\n    (true :positive)))\n(classify 5)"),
        ":positive"
    );
}

// --- Phase 1: strings + error handling ---

#[test]
fn test_string_literal() {
    assert_eq!(eval("\"hello\""), "hello");
}

#[test]
fn test_string_concat() {
    assert_eq!(eval("(string \"hello\" \" \" \"world\")"), "hello world");
}

#[test]
fn test_string_size() {
    assert_eq!(eval("(string/size-of \"hello\")"), "5");
}

// --- Phase 1: recursion ---

#[test]
fn test_recursive_factorial() {
    // defn at top-level uses letrec, enabling recursion
    assert_eq!(
        eval("(defn fact [n] (if (<= n 1) 1 (* n (fact (- n 1)))))\n(fact 5)"),
        "120"
    );
}

#[test]
fn test_closure_with_if() {
    // Closure body contains if/else — tests branch emission in closure context
    assert_eq!(eval("(defn abs [x] (if (< x 0) (- 0 x) x))\n(abs -5)"), "5");
}

#[test]
fn test_closure_with_let_and_if() {
    assert_eq!(
        eval("(defn clamp [x lo hi] (if (< x lo) lo (if (> x hi) hi x)))\n(clamp 15 0 10)"),
        "10"
    );
}

#[test]
fn test_mutual_recursion() {
    assert_eq!(
        eval("(defn even? [n] (if (= n 0) true (odd? (- n 1))))\n(defn odd? [n] (if (= n 0) false (even? (- n 1))))\n(even? 10)"),
        "true"
    );
}

#[test]
fn test_recursive_sum() {
    assert_eq!(
        eval("(defn sum [n] (if (<= n 0) 0 (+ n (sum (- n 1)))))\n(sum 10)"),
        "55"
    );
}

#[test]
fn test_let_with_if() {
    assert_eq!(eval("(let* [[x (if true 10 20)]] (+ x 5))"), "15");
}

// --- Nested closure calls (env stack regression) ---

#[test]
fn test_higher_order_with_capture() {
    // apply-twice captures a letrec binding (itself) and calls f, which
    // overwrites the env. After f returns, the recursive LoadCapture must
    // still read the correct self-reference.
    assert_eq!(
        eval("(defn apply-twice [f x] (f (f x)))\n(apply-twice (fn [n] (+ n 1)) 0)"),
        "2"
    );
}

#[test]
fn test_map_over_list() {
    // map recurses and calls the user closure at each step — the classic
    // env-stack-corruption pattern that motivated the stack allocator.
    assert_eq!(
        eval(concat!(
            "(defn map [f lst]\n",
            "  (if (empty? lst) ()\n",
            "    (cons (f (first lst)) (map f (rest lst)))))\n",
            "(map (fn [x] (* x x)) (list 1 2 3))"
        )),
        "(1 4 9)"
    );
}

#[test]
fn test_capture_read_after_nested_call() {
    // g captures both f and h. It calls f (overwriting env), then must
    // still read h from its capture slot.
    assert_eq!(
        eval(concat!(
            "(defn f [x] (+ x 10))\n",
            "(defn h [x] (* x 2))\n",
            "(defn g [x] (h (f x)))\n",
            "(g 5)"
        )),
        "30"
    );
}

#[test]
fn test_closure_let_binding() {
    // Simpler: lambda called immediately
    assert_eq!(eval("((fn [] (let* [[x 42]] x)))"), "42");
}

#[test]
fn test_closure_let_with_call() {
    assert_eq!(eval("((fn [a] (let* [[b (+ a 10)]] b)) 5)"), "15");
}

#[test]
fn test_closure_let_defn() {
    assert_eq!(eval("(defn f [] (let* [[x 42]] x))\n(f)"), "42");
}

#[test]
fn test_dump_closure_let_lir() {
    let mut vm = elle::VM::new();
    let mut symbols = Box::new(elle::SymbolTable::new());
    elle::register_primitives(&mut vm, &mut symbols);
    let sym_ptr: *mut elle::SymbolTable = &mut *symbols;
    elle::context::set_symbol_table(sym_ptr);
    elle::primitives::set_length_symbol_table(sym_ptr);
    let lir =
        elle::pipeline::compile_file_to_lir("((fn [] (let* [[x 42]] x)))", &mut symbols, "<test>")
            .unwrap();
    eprintln!(
        "Entry: num_regs={} num_locals={} num_captures={} num_params={}",
        lir.num_regs, lir.num_locals, lir.num_captures, lir.num_params
    );
    for block in &lir.blocks {
        eprintln!("Block {:?}:", block.label);
        for si in &block.instructions {
            eprintln!("  {:?}", si.instr);
        }
        eprintln!("  term: {:?}", block.terminator);
    }
    // Find nested closures
    for block in &lir.blocks {
        for si in &block.instructions {
            if let elle::lir::LirInstr::MakeClosure { func, .. } = &si.instr {
                eprintln!(
                    "\nClosure: num_regs={} num_locals={} num_captures={} num_params={}",
                    func.num_regs, func.num_locals, func.num_captures, func.num_params
                );
                for b in &func.blocks {
                    eprintln!("  Block {:?}:", b.label);
                    for s in &b.instructions {
                        eprintln!("    {:?}", s.instr);
                    }
                    eprintln!("    term: {:?}", b.terminator);
                }
            }
        }
    }
}

// --- Tail calls ---

#[test]
fn test_tail_call_deep_recursion() {
    // 100K iterations — would overflow without tail calls
    assert_eq!(
        eval("(letrec ([f (fn [n] (if (= n 0) 42 (f (- n 1))))]) (f 100000))"),
        "42"
    );
}

#[test]
fn test_tail_call_mutual_recursion() {
    assert_eq!(
        eval(
            "(letrec ([even (fn [n] (if (= n 0) true (odd (- n 1))))]
                      [odd  (fn [n] (if (= n 0) false (even (- n 1))))])
               (even 10000))"
        ),
        "true"
    );
}

#[test]
fn test_tail_call_accumulator() {
    assert_eq!(
        eval(
            "(letrec ([sum (fn [n acc] (if (= n 0) acc (sum (- n 1) (+ acc n))))]) (sum 10000 0))"
        ),
        "50005000"
    );
}

// --- Float arithmetic ---

#[test]
fn test_float_division() {
    assert_eq!(eval("(/ 7.0 2)"), "3.5");
}

#[test]
fn test_float_addition() {
    assert_eq!(eval("(+ 1.5 2.5)"), "4");
}

#[test]
fn test_int_float_promotion() {
    assert_eq!(eval("(+ 1 2.5)"), "3.5");
}

#[test]
fn test_float_comparison() {
    assert_eq!(eval("(> 3.14 2.71)"), "true");
}
