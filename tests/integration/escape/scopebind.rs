use super::*;

#[test]
fn correct_scope_binding_with_immediate_init() {
    // Scope binding x holds 42 (immediate), returned directly
    eval_source("(let [x 42] x)", |r| assert_eq!(r.unwrap(), Value::int(42)));
}

// ── Correctness: Tier 4 nested let/block returns correct values ─────

#[test]
fn correct_nested_let_with_length() {
    eval_source("(let [x (list 1 2 3)] (let [y (length x)] y))", |r| {
        assert_eq!(r.unwrap(), Value::int(3))
    });
}

#[test]
fn correct_nested_block_with_arithmetic() {
    eval_source("(let [x 10] (block (+ x 5)))", |r| {
        assert_eq!(r.unwrap(), Value::int(15))
    });
}

#[test]
fn correct_deeply_nested_let() {
    eval_source("(let [x 1] (let [y 2] (let [z 3] (+ x (+ y z)))))", |r| {
        assert_eq!(r.unwrap(), Value::int(6))
    });
}

// ── Regression: unsafe patterns must produce correct results ────────
//
// These verify that the analysis correctly REJECTS patterns that would
// be use-after-free if scope-allocated. The programs must work correctly
// (values are NOT freed because scope allocation was not applied).

#[test]
fn regression_returned_binding_not_freed() {
    eval_source(
        "(def result (let [x (list 1 2 3)] x)) (length result)",
        |r| {
            let result = r.unwrap();
            assert_eq!(result, Value::int(3));
        },
    );
}

#[test]
fn regression_global_set_not_freed() {
    eval_source(
        "(var holder nil)
         (let [x (list 1 2 3)]
           (assign holder x)
           42)
         (length holder)",
        |r| {
            let result = r.unwrap();
            assert_eq!(result, Value::int(3));
        },
    );
}

#[test]
fn regression_captured_binding_not_freed() {
    eval_source(
        "(def make-getter
           (fn ()
             (let [data (list 1 2 3)]
               (fn () data))))
         (def getter (make-getter))
         (length (getter))",
        |r| {
            let result = r.unwrap();
            assert_eq!(result, Value::int(3));
        },
    );
}

#[test]
fn regression_yielded_value_not_freed() {
    eval_source(
        "(def gen (fn () (let [x (list 1 2 3)] (yield x) nil)))
         (def f (fiber/new gen 2))
         (def yielded (fiber/resume f))
         (length yielded)",
        |r| {
            let result = r.unwrap();
            assert_eq!(result, Value::int(3));
        },
    );
}

// ── Stress: allocation-heavy programs with scope allocation ─────────

#[test]
fn stress_loop_with_scope_allocation() {
    // Tight loop where each iteration scope-allocates and releases.
    // Note: the let body does `(assign i ...)` which is an outward set.
    // The let itself does NOT scope-allocate (Set in result position is
    // conservatively unsafe in result_is_safe). However, the implicit
    // while-block DOES scope-allocate since Tier 8 recognizes the outward
    // set value `(+ a b)` as immediate.
    eval_source(
        "(var i 0)
         (while (< i 1000)
           (let [a i b (+ i 1)]
             (assign i (+ a b))))",
        |r| {
            let result = r.unwrap();
            assert!(result.is_nil());
        },
    );
}

#[test]
fn stress_nested_scope_allocation() {
    eval_source(
        "(var sum 0)
         (var i 0)
         (while (< i 100)
           (let [a i]
             (let [b (+ a 1)]
               (assign sum (+ sum (+ a b)))))
           (assign i (+ i 1)))
         sum",
        |r| {
            let result = r.unwrap();
            // sum = Σ(i=0 to 99) of (i + i+1) = Σ(2i+1) = 2*4950 + 100 = 10000
            assert_eq!(result, Value::int(10000));
        },
    );
}

// ── Correctness of break with scoped blocks ─────────────────────────

#[test]
fn break_from_nested_scoped_let_correct() {
    // Inner let qualifies for scope allocation.
    // Break exits the block, compensating exits fire for the inner let's scope.
    eval_source(
        "(block :done
           (let [x 10]
             (let [y 20]
               (break :done (+ x y)))))",
        |r| {
            let result = r.unwrap();
            assert_eq!(result, Value::int(30));
        },
    );
}

// ── Tier 7 correctness: inner break with scope allocation ───────────

#[test]
fn correct_let_with_inner_block_break() {
    // Let scope-allocates; inner block break stays within the let's scope.
    eval_source(
        "(let [x 42]
           (block :inner
             (if (> x 10) (break :inner (+ x 1)) (- x 1))))",
        |r| {
            let result = r.unwrap();
            assert_eq!(result, Value::int(43));
        },
    );
}

#[test]
fn correct_let_with_while_break() {
    // Let scope-allocates; while-break targets the implicit while-block.
    eval_source(
        "(let [n 100]
           (var i 0)
           (while (< i n)
             (if (= i 5) (break :while i))
             (assign i (+ i 1)))
           i)",
        |r| {
            let result = r.unwrap();
            assert_eq!(result, Value::int(5));
        },
    );
}

#[test]
fn correct_let_with_inner_break_returns_last_expr() {
    // Break exits inner block early; let body continues to final expression.
    eval_source(
        "(let [x 10]
           (block :skip (break :skip 0))
           (+ x 5))",
        |r| {
            let result = r.unwrap();
            assert_eq!(result, Value::int(15));
        },
    );
}

// ── Correctness of while/break with scope allocation ────────────────

#[test]
fn correct_while_in_scoped_let() {
    eval_source(
        "(var sum 0)
         (let [@x 10]
           (while (> x 0)
             (assign sum (+ sum x))
             (assign x (- x 1)))
           nil)
         sum",
        |r| {
            let result = r.unwrap();
            assert_eq!(result, Value::int(55));
        },
    );
}

#[test]
fn correct_block_with_safe_break_in_scope() {
    eval_source(
        "(block :done
           (if true (break :done 42) 0))",
        |r| {
            let result = r.unwrap();
            assert_eq!(result, Value::int(42));
        },
    );
}

#[test]
fn correct_block_with_while_and_break() {
    eval_source(
        "(var i 0)
         (block :loop
           (while (< i 10)
             (if (= i 5) (break :loop :found))
             (assign i (+ i 1)))
           :not-found)",
        |r| {
            let result = r.unwrap();
            assert_eq!(result, Value::keyword("found"));
        },
    );
}

#[test]
fn correct_while_as_let_body() {
    // while is the entire let body — result is nil
    eval_source("(let [x 1] (while false x))", |r| {
        let result = r.unwrap();
        assert_eq!(result, Value::NIL);
    });
}

#[test]
fn assign_to_global_preserves_value_via_first() {
    // Like regression_global_set_not_freed but reads the head of `holder`
    // via `first` (which returns an immediate int). The first of the first
    // pair in the list should be `10`, regardless of whether the tail
    // cells are corrupted. If the first is nil instead, the VERY FIRST pair
    // was overwritten/freed. If the first is 10 but `length` still returns
    // 1, only the tail (rest chain) was corrupted.
    eval_source(
        "(var holder nil)
         (let [x (list 10 20 30)]
           (assign holder x)
           42)
         (first holder)",
        |r| {
            let result = r.unwrap();
            assert_eq!(result, Value::int(10));
        },
    );
}
