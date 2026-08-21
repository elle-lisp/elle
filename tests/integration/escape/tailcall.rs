use super::*;

// ── Correctness: tail-call scope allocation produces correct values ─

#[test]
fn correct_tail_recursive_loop_with_let() {
    // Simple tail-recursive countdown with let binding.
    eval_source(
        "(defn loop (n acc)
               (if (<= n 0) acc
                 (let [s (concat \"iter\" (number->string n))]
                   (loop (- n 1) (+ acc 1)))))
             (loop 100 0)",
        |r| assert_eq!(r.unwrap(), Value::int(100)),
    );
}

#[test]
fn correct_mutual_tail_recursion_with_let() {
    // Mutual tail recursion — both functions have let bindings.
    eval_source(
        "(defn even-f (n)
               (if (<= n 0) :even
                 (let [s (concat \"e\" (number->string n))]
                   (odd-f (- n 1)))))
             (defn odd-f (n)
               (if (<= n 0) :odd
                 (let [s (concat \"o\" (number->string n))]
                   (even-f (- n 1)))))
             (even-f 10)",
        |r| assert_eq!(r.unwrap(), Value::keyword("even")),
    );
}

#[test]
fn correct_tail_call_with_nested_lets() {
    // Nested lets, both scope-allocated, tail call in innermost body.
    eval_source(
        "(defn loop (n)
               (if (<= n 0) :done
                 (let [a (concat \"a\" (number->string n))]
                   (let [b (concat \"b\" (number->string n))]
                     (loop (- n 1))))))
             (loop 50)",
        |r| assert_eq!(r.unwrap(), Value::keyword("done")),
    );
}

#[test]
fn correct_tail_call_if_both_branches() {
    // Both if branches are tail calls. Scope allocation must work
    // correctly regardless of which branch executes.
    eval_source(
        "(defn classify (n)
               (let [s (concat \"checking\" (number->string n))]
                 (if (<= n 0)
                   (base-case n)
                   (classify (- n 1)))))
             (defn base-case (n) (* n 2))
             (classify 10)",
        |r| assert_eq!(r.unwrap(), Value::int(0)),
    );
}

#[test]
fn call_scoped_correct_nqueens_pattern() {
    // Simplified nqueens: search receives (pair col queens) and returns
    // an integer. The pair cell should be freed after search returns.
    // Without safe? check this counts all placements (5^5 = 3125).
    eval_source(
        r#"(letrec
            [search (fn [n row queens count]
              (if (= row n) (+ count 1)
                (try-col n 0 queens row count))) try-col (fn [n col queens row count]
              (if (= col n) count
                (try-col n (+ col 1) queens row
                  (search n (+ row 1) (pair col queens) count))))]
            (search 5 0 (list) 0))
        "#,
        // eval_source — uses stdlib + and pair
        |r| assert_eq!(r.unwrap(), Value::int(3125)),
    );
}

#[test]
fn call_scoped_mutual_recursion_result_immediate() {
    // Mutual recursion where both functions return immediates.
    // Compilation succeeds (fixpoint converges).
    let source = r#"(letrec
        [even-count (fn [n] (if (%le n 0) 0 (odd-count (%sub n 1)))) odd-count (fn [n] (if (%le n 0) 0 (%add 1 (even-count (%sub n 1)))))]
        nil)
    "#;
    let mut symbols = SymbolTable::new();
    let compiled = compile(source, &mut symbols, "<test>").expect("compilation failed");
    assert!(!compiled.bytecode.instructions.is_empty());
}

#[test]
fn correct_scope_callee_not_freed_before_tail_call() {
    // Regression test: when the tail-call callee is a scope binding,
    // scope allocation must NOT happen (the callee would be freed).
    // This pattern compiles and runs: the scope callee survives the call.
    eval_source(
        "(assert (= ((fn (&keys opts)
                 (let [f (fn () opts)]
                   (f)))
               :x 10) {:x 10}) \"keys mutable capture\")",
        |r| assert_eq!(r.unwrap(), Value::bool(true)),
    ); // assert returns true on success
}

// The per-function `rotation_safe` flag (and `LirFunction::rotation_safe`) was
// retired in the s11 escape-analysis overhaul. Frame reuse for tail recursion is
// now a consequence of region inference — a recursive scope that does not escape
// mints no region — and there is no single field that re-expresses the old
// boolean. The three tests below therefore assert the *user-visible* guarantees
// that rotation safety used to provide, end-to-end (the migration path the
// original FIXME spelled out). Each is counter-factually sharp: if frame reuse
// regressed, the deep calls would overflow rather than return a value.

#[test]
fn rotation_safe_for_pure_recursive_functions() {
    // A pure tail-recursive function reuses its frame, so a million-deep call
    // completes in bounded stack instead of overflowing.
    eval_source(
        "(defn f (n) (if (%le n 0) n (f (%sub n 1)))) (f 1000000)",
        |r| {
            assert_eq!(
                r.unwrap(),
                Value::int(0),
                "pure tail recursion must reuse its frame: 1e6-deep call should return 0, not overflow"
            )
        },
    );
}

#[test]
fn rotation_unsafe_for_push_in_body() {
    // Counterpart to the pure case: when the recursive body mutates a collection
    // that OUTLIVES the call (`acc`, bound outside the recursion), those
    // allocations must NOT be scope-reclaimed across the tail call — the old
    // "rotation-unsafe" condition. End-to-end, every escaping push must survive,
    // so 2000 deep iterations leave exactly 2000 elements. (The original
    // internal-state source `(push @[] 1)` is now a lowerer poison node — `push`
    // needs a real place — so the property is exercised with a valid escaping
    // accumulator instead.)
    eval_source(
        "(defn build (n acc) (if (%le n 0) acc (begin (push acc n) (build (%sub n 1) acc)))) (let [a @[]] (build 2000 a) (length a))",
        |r| {
            assert_eq!(
                r.unwrap(),
                Value::int(2000),
                "pushes into an escaping accumulator must survive the recursion (the rotation-unsafe scope must not be reclaimed)"
            )
        },
    );
}

#[test]
fn rotation_safe_for_mutual_recursion() {
    // Mutual tail recursion between pure functions enjoys the same frame reuse as
    // self recursion (both legs are rotation-safe): a 100k-deep even?/odd?
    // ping-pong completes in bounded stack.
    eval_source(
        "(defn even? (n) (if (%le n 0) true (odd? (%sub n 1)))) (defn odd? (n) (if (%le n 0) false (even? (%sub n 1)))) (even? 100000)",
        |r| {
            assert_eq!(
                r.unwrap(),
                Value::bool(true),
                "pure mutual tail recursion must reuse frames: 100k-deep even? should return true, not overflow"
            )
        },
    );
}
