use super::*;

// ── Tier 8 correctness: programs with immediate outward set ─────────

#[test]
fn correct_outward_set_immediate_in_scope() {
    // Verify the assign actually takes effect when scope-allocated.
    eval_source(
        "(begin
           (var counter 0)
           (let [temp (list 1 2 3)]
             (assign counter (+ counter 1))
             (length temp))
           counter)",
        |r| {
            let result = r.unwrap();
            assert_eq!(result, Value::int(1));
        },
    );
}

#[test]
fn correct_outward_set_bool_in_scope() {
    eval_source(
        "(begin
           (var flag false)
           (let [temp (list 1 2 3)]
             (assign flag true)
             (length temp))
           flag)",
        |r| {
            let result = r.unwrap();
            assert_eq!(result, Value::TRUE);
        },
    );
}

#[test]
fn correct_loop_with_outward_set_counter() {
    // Loop where each iteration scope-allocates and sets an outer counter.
    eval_source(
        "(begin
           (var total 0)
           (var i 0)
           (while (< i 100)
             (let [temp @[1 2 3]]
               (assign total (+ total (length temp)))
               (assign i (+ i 1))))
           total)",
        |r| {
            let result = r.unwrap();
            assert_eq!(result, Value::int(300));
        },
    );
}

#[test]
fn correct_inner_let_set_own_binding() {
    // Inner let sets its own binding — must work correctly.
    eval_source(
        "(let [x 10]
           (let [@y 5]
             (assign y (+ y x))
             (+ x y)))",
        |r| {
            let result = r.unwrap();
            assert_eq!(result, Value::int(25));
        },
    );
}

// ── Correctness: programs with scope allocation produce correct results ─

#[test]
fn correct_arithmetic_in_scope() {
    eval_source("(let [a 1 b 2 c 3] (+ a (+ b c)))", |r| {
        assert_eq!(r.unwrap(), Value::int(6))
    });
}

#[test]
fn correct_nested_scope() {
    eval_source("(let [x 4] (let [y 6] (+ x y)))", |r| {
        assert_eq!(r.unwrap(), Value::int(10))
    });
}

#[test]
fn correct_comparison_in_scope() {
    eval_source("(let [a 10 b 20] (< a b))", |r| {
        assert_eq!(r.unwrap(), Value::TRUE)
    });
}

#[test]
fn correct_if_with_scope() {
    eval_source("(let [x 5] (if (> x 3) 1 0))", |r| {
        assert_eq!(r.unwrap(), Value::int(1))
    });
}

#[test]
fn correct_letrec_fibonacci() {
    eval_source(
        "(letrec [fib (fn (n)
                           (if (<= n 1) n
                               (+ (fib (- n 1)) (fib (- n 2)))))]
               (fib 10))",
        |r| assert_eq!(r.unwrap(), Value::int(55)),
    );
}

#[test]
fn correct_block_scope() {
    eval_source("(block :done (+ 10 20))", |r| {
        assert_eq!(r.unwrap(), Value::int(30))
    });
}

#[test]
fn correct_deeply_nested_scopes() {
    eval_source(
        "(let [a 1]
               (let [b 2]
                 (let [c 3]
                   (+ a (+ b c)))))",
        |r| assert_eq!(r.unwrap(), Value::int(6)),
    );
}

// ── Correctness: Tier 1 primitive whitelist produces correct results ─

#[test]
fn correct_length_in_scope() {
    eval_source("(let [x (list 1 2 3)] (length x))", |r| {
        assert_eq!(r.unwrap(), Value::int(3))
    });
}

#[test]
fn correct_empty_in_scope() {
    eval_source("(let [x (list 1 2 3)] (empty? x))", |r| {
        assert_eq!(r.unwrap(), Value::FALSE)
    });
}

#[test]
fn correct_type_in_scope() {
    eval_source(r#"(let [x "hello"] (type x))"#, |r| {
        assert_eq!(r.unwrap(), Value::keyword("string"))
    });
}

#[test]
fn correct_abs_in_scope() {
    eval_source("(let [x -42] (abs x))", |r| {
        assert_eq!(r.unwrap(), Value::int(42))
    });
}

#[test]
fn correct_floor_in_scope() {
    eval_source("(let [x 3.7] (floor x))", |r| {
        assert_eq!(r.unwrap(), Value::int(3))
    });
}

#[test]
fn correct_equality_check_in_scope() {
    eval_source("(let [x 42] (= x 42))", |r| {
        assert_eq!(r.unwrap(), Value::TRUE)
    });
}

#[test]
fn correct_unary_minus_in_scope() {
    eval_source("(let [x 42] (- x))", |r| {
        assert_eq!(r.unwrap(), Value::int(-42))
    });
}

// ── Correctness: Tier 3 outer-Var returns correct values ────────────

#[test]
fn correct_outer_binding_returned_from_scope() {
    // Inner let does work with temp, returns outer binding x
    eval_source("(let [x 42] (let [temp (list 1 2 3)] x))", |r| {
        assert_eq!(r.unwrap(), Value::int(42))
    });
}

// ── Correctness: Tier 5 match produces correct results ─────────────

#[test]
fn correct_match_in_scope_keyword_result() {
    eval_source("(let [x 1] (match x 0 :zero 1 :one _ :other))", |r| {
        assert_eq!(r.unwrap(), Value::keyword("one"))
    });
}

#[test]
fn correct_match_in_scope_int_result() {
    eval_source("(let [x 2] (match x 0 0 1 10 _ -1))", |r| {
        assert_eq!(r.unwrap(), Value::int(-1))
    });
}

#[test]
fn correct_match_in_scope_with_intrinsic() {
    eval_source("(let [x 0 y 5] (match x 0 (+ y 10) _ (- y 1)))", |r| {
        assert_eq!(r.unwrap(), Value::int(15))
    });
}

// ── Correctness: Tier 6 while produces correct results ─────────────

#[test]
fn correct_while_in_scope_returns_nil() {
    eval_source("(let [x 1] (while false 42))", |r| {
        assert!(r.unwrap().is_nil())
    });
}
