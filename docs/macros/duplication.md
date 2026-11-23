# What a duplicated body costs a compile (2026-11-23)

<!-- audited: 2026-11-23 -->

How compile time grows when nested `each` loops copy their body into every arm, against a loop that places its body once.

**Verdict: the prelude's `each` multiplies its expansion by more than four at
each level of nesting, and six nested loops take 2.3 s to compile. A loop that
places its body once takes 0.02 s.**

[types.md](types.md) states the rule this report informed.

## What is measured

The subject is the release build of `971509475`. Each program defines one
function that nests loops over one sequence around one `println`, and calls it
once. A time is the fastest of three runs of `elle` on the program, in
seconds, and an empty program takes 0.01.

| Depth | `each`, the caller passes an array | `each` over a literal array | Body placed once | `while` by hand |
|-------|------|------|------|------|
| 1 | 0.01 | 0.01 | 0.01 | 0.01 |
| 2 | 0.01 | 0.01 | 0.01 | 0.01 |
| 3 | 0.02 | 0.02 | 0.01 | 0.01 |
| 4 | 0.05 | 0.03 | 0.01 | 0.01 |
| 5 | 0.27 | 0.14 | 0.01 | 0.01 |
| 6 | 2.31 | 0.96 | 0.02 | 0.01 |

Twelve nested loops with the body placed once compile in 0.16 s.

The expansion, in nodes of the datum `expand-macro` answers:

| Depth | `each` | Body placed once |
|-------|--------|------------------|
| 1 | 431 | 65 |
| 2 | 2,135 | 125 |
| 3 | 8,951 | 185 |
| 4 | 36,215 | 245 |

The prelude's `each` holds its body in four arms. A type the compiler can
prove does not avoid the cost. The literal
array grows as the parameter does, because the fold deletes the dead arms
after every earlier pass has read them.

## The loop that places its body once

The loop reads its sequence through four functions, and each function
dispatches on the type. It covers a list and the indexed sequences, which is
enough to measure:

```lisp
(def protocol "
(defn start [s] (match (type-of s) :list s _ 0))
(defn more? [s at] (match (type-of s) :list (pair? at) _ (< at (length s))))
(defn item [s at] (match (type-of s) :list (first at) _ (get s at)))
(defn step [s at] (match (type-of s) :list (rest at) _ (+ at 1)))
(defmacro each-once (var in seq & body)
  `(let [s ,seq]
     (let [@at (start s)]
       (while (more? s at)
         (let [,var (item s at)] ,;body)
         (assign at (step s at))))))")

(defn run [program]
  "The value of program, compiled after the protocol as one file."
  ((get (get (compile/whole-module (string protocol "\n" program) "<doc>") 0) 1)))

(defn type-tests [program]
  "How many type-of tests the fold leaves in program and the protocol."
  (let [text (get (compile/dumps (string protocol "\n" program) "<doc>") :fhir)]
    (- (length (string/split text "type-of")) 1)))

(def sum "(defn sum [a b]
            (def @n 0)
            (each-once x in a (each-once y in b (assign n (+ n (* x y)))))
            n)")

(assert (= 180 (run (string sum "\n(sum [1 2 3] [10 20])"))))

(def visit "(defn visit [a b]
              (each-once x in a (each-once y in b (println x y))))")

(assert (= 0 (type-tests (string visit "\n(visit [1 2 3] [10 20])")))
        "every caller passes arrays, so the fold leaves no test")
(assert (< 0 (type-tests (string visit "\n(visit [1 2] [3]) (visit (list 1) (list 2))")))
        "a list caller keeps the tests")
```

With a list as the inner sequence, this loop sums a wrong answer on the
measured build (#1558).
