use super::*;

// --- Phase 2: yield/resume ---

#[test]
fn test_basic_yield() {
    // A fiber that yields 42, then returns 99
    assert_eq!(
        eval(concat!(
            "(let* [f (fiber/new (fn [] (yield 42) 99) |:yield|)]\n",
            "  (fiber/resume f))"
        )),
        "42"
    );
}

#[test]
fn test_resume_tail_vs_nontail_position() {
    // A bare `(fiber/resume f)` in TAIL position compiles to a tail call, which
    // the host dispatches through `rt_prepare_tail_call`; the same call bound in
    // a `let` (non-tail) dispatches through `rt_call`. BOTH host paths must drive
    // the fiber via `handle_fiber_resume` on the SIG_RESUME the primitive returns.
    // When only `rt_call` handled it, a tail-position resume returned the fiber
    // itself ("<fiber:new>") instead of the yielded value — the failure this pins.
    assert_eq!(
        eval("(let* [f (fiber/new (fn [] (yield 7) 9) |:yield|)]\n  (fiber/resume f))"),
        "7",
        "tail-position (fiber/resume f)"
    );
    assert_eq!(
        eval(concat!(
            "(let* [f (fiber/new (fn [] (yield 7) 9) |:yield|)]\n",
            "  (let* [a (fiber/resume f)] (+ a 0)))"
        )),
        "7",
        "non-tail (fiber/resume f) bound in a let"
    );
}

#[test]
fn test_yield_resume_value() {
    // Resume a fiber with a value; the yield expression evaluates to it
    assert_eq!(
        eval(concat!(
            "(let* [f (fiber/new (fn [] (+ 1 (yield 0))) |:yield|)]\n",
            "  (fiber/resume f)\n",   // yields 0, fiber paused
            "  (fiber/resume f 10))"  // resumes with 10, returns 1+10=11
        )),
        "11"
    );
}

#[test]
fn test_multiple_yields() {
    // Fiber yields 1, 2, 3 in sequence
    assert_eq!(
        eval(concat!(
            "(let* [f (fiber/new (fn [] (yield 1) (yield 2) 3) |:yield|)]\n",
            "  (let* [a (fiber/resume f)\n",
            "         b (fiber/resume f)\n",
            "         c (fiber/resume f)]\n",
            "    (+ a (+ b c))))"
        )),
        "6"
    );
}

#[test]
fn test_fiber_dead_after_return() {
    // After a fiber returns, it's dead
    assert_eq!(
        eval(concat!(
            "(let* [f (fiber/new (fn [] 42) |:yield|)]\n",
            "  (fiber/resume f))"
        )),
        "42"
    );
}

#[test]
fn test_yield_through_call() {
    // A calls B, B yields — yield propagates through A to the fiber
    assert_eq!(
        eval(concat!(
            "(defn inner [] (yield 77))\n",
            "(defn outer [] (+ 1 (inner)))\n",
            "(let* [f (fiber/new outer |:yield|)]\n",
            "  (fiber/resume f))"
        )),
        "77"
    );
}

#[test]
fn test_yield_through_call_resume() {
    // A calls B, B yields. Resume B with a value, B returns it,
    // A adds 1 to the result.
    assert_eq!(
        eval(concat!(
            "(defn inner [] (yield 0))\n",
            "(defn outer [] (+ 1 (inner)))\n",
            "(let* [f (fiber/new outer |:yield|)]\n",
            "  (fiber/resume f)\n",   // yields 0
            "  (fiber/resume f 10))"  // resumes inner with 10, outer returns 11
        )),
        "11"
    );
}

#[test]
fn test_generator_pattern() {
    // Generator: yields successive values
    assert_eq!(
        eval(concat!(
            "(let* [g (fiber/new (fn []\n",
            "           (yield 10)\n",
            "           (yield 20)\n",
            "           (yield 30)\n",
            "           0) |:yield|)]\n",
            "  (let* [a (fiber/resume g)\n",
            "         b (fiber/resume g)\n",
            "         c (fiber/resume g)]\n",
            "    (+ a (+ b c))))"
        )),
        "60"
    );
}

#[test]
fn test_yield_in_loop() {
    // Yield inside a recursive loop — yields 1, 2, 3
    assert_eq!(
        eval(concat!(
            "(defn count-up [n max]\n",
            "  (if (> n max) nil\n",
            "    (let* [_ (yield n)] (count-up (+ n 1) max))))\n",
            "(let* [f (fiber/new (fn [] (count-up 1 3)) |:yield|)]\n",
            "  (let* [a (fiber/resume f)\n",
            "         b (fiber/resume f)\n",
            "         c (fiber/resume f)]\n",
            "    (+ a (+ b c))))"
        )),
        "6"
    );
}

// --- Phase 2: resume chain regression tests ---

#[test]
fn test_three_sequential_yields_through_call() {
    // Three sequential yields through a wrapper function — the println bug pattern.
    // Each call to `inner` yields, creating a yield-through-call chain.
    // On resume, the outer closure must re-yield for subsequent calls.
    assert_eq!(
        eval(concat!(
            "(defn inner [x] (yield x))\n",
            "(let* [f (fiber/new (fn [] (inner 1) (inner 2) (inner 3) :done) |:yield|)]\n",
            "  (let* [a (fiber/resume f)\n",
            "         b (fiber/resume f)\n",
            "         c (fiber/resume f)\n",
            "         d (fiber/resume f)]\n",
            "    (list a b c d)))"
        )),
        "(1 2 3 :done)"
    );
}

#[test]
fn test_five_sequential_yields_through_call() {
    // Five yields through a call — stress the CPS resume block emission.
    assert_eq!(
        eval(concat!(
            "(defn inner [x] (yield x))\n",
            "(let* [f (fiber/new (fn []\n",
            "         (inner 10) (inner 20) (inner 30) (inner 40) (inner 50)\n",
            "         :done) |:yield|)]\n",
            "  (let* [a (fiber/resume f)\n",
            "         b (fiber/resume f)\n",
            "         c (fiber/resume f)\n",
            "         d (fiber/resume f)\n",
            "         e (fiber/resume f)\n",
            "         z (fiber/resume f)]\n",
            "    (list a b c d e z)))"
        )),
        "(10 20 30 40 50 :done)"
    );
}

#[test]
fn test_yield_through_deep_call_stack() {
    // Yield propagates through 3 levels of call stack
    assert_eq!(
        eval(concat!(
            "(defn level3 [] (yield 99))\n",
            "(defn level2 [] (+ 1 (level3)))\n",
            "(defn level1 [] (+ 10 (level2)))\n",
            "(let* [f (fiber/new level1 |:yield|)]\n",
            "  (fiber/resume f))"
        )),
        "99"
    );
}

#[test]
fn test_yield_through_deep_resume() {
    // Resume with a value through 3-level call stack
    assert_eq!(
        eval(concat!(
            "(defn level3 [] (yield 0))\n",
            "(defn level2 [] (+ 1 (level3)))\n",
            "(defn level1 [] (+ 10 (level2)))\n",
            "(let* [f (fiber/new level1 |:yield|)]\n",
            "  (fiber/resume f)\n",
            "  (fiber/resume f 5))"
        )),
        "16"
    );
}

#[test]
fn test_nested_fiber_resume() {
    // fiber/resume inside another fiber's execution
    assert_eq!(
        eval(concat!(
            "(let* [inner (fiber/new (fn [] (yield 42) 99) |:yield|)\n",
            "       outer (fiber/new (fn []\n",
            "                (let* [a (fiber/resume inner)]\n",
            "                  (+ a (fiber/resume inner)))) |:yield|)]\n",
            "  (fiber/resume outer))"
        )),
        "141"
    );
}

#[test]
fn test_yield_through_call_multiple_resumes() {
    // Multiple yields-through-call with resume values
    assert_eq!(
        eval(concat!(
            "(defn helper [x] (+ x (yield x)))\n",
            "(let* [f (fiber/new (fn []\n",
            "         (let* [a (helper 1)\n",
            "                b (helper 2)]\n",
            "           (+ a b))) |:yield|)]\n",
            "  (fiber/resume f)\n",    // yields 1
            "  (fiber/resume f 10)\n", // helper returns 1+10=11, then yields 2
            "  (fiber/resume f 20))"   // helper returns 2+20=22, result = 11+22=33
        )),
        "33"
    );
}
