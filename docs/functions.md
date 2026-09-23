# Functions

<!-- audited: 2026-09-23 -->

How to make a function with `fn` and `defn`, collect arguments, close over
state, pass functions around, and how deep recursion may go.

## fn — anonymous functions

`fn` creates a closure. Brackets delimit the parameter list. Parameters
are immutable by default; prefix with `@` to allow mutation via `assign`.

```lisp
(def double (fn [x] (* x 2)))
(assert (= 42 (double 21)))

# A mutable parameter.
(def bump (fn [@n] (assign n (+ n 1)) n))
(assert (= 11 (bump 10)))
```

## defn — named functions

`defn` is sugar for `(def name (fn [params] body))`. It supports a
docstring as the first body form.

```lisp
(defn letter-grade [score]
  "Convert a numeric score to a letter grade."
  (cond
    (>= score 90) "A"
    (>= score 80) "B"
    (>= score 70) "C"
    (>= score 60) "D"
    "F"))

(assert (= "A" (letter-grade 95)))
(assert (= "F" (letter-grade 55)))
(assert (= "Convert a numeric score to a letter grade." (doc letter-grade)))
```

## Variadic functions

`&` collects remaining arguments into a list. `&rest` is a synonym for
`&` — here and everywhere a rest collector is accepted (destructuring
patterns, `match` patterns, `defmacro` parameter lists).

```lisp
(defn sum [& nums]
  (fold + 0 nums))
(assert (= 10 (sum 1 2 3 4)))

(defn product [&rest nums]
  (fold * 1 nums))
(assert (= 24 (product 1 2 3 4)))

(defn collected [& xs] xs)
(assert (= :list (type-of (collected 1 2))))
```

## Closures

Functions capture their lexical environment. The captured values persist
as long as the closure does.

```lisp
(defn make-counter []
  (var n 0)
  (fn []
    (assign n (+ n 1))
    n))

(def counter (make-counter))
(assert (= 1 (counter)))
(assert (= 2 (counter)))
(assert (= 3 (counter)))
```

## Higher-order functions

`map` and `filter` keep the collection's type: an array gives an array, a
list gives a list.

```lisp
(assert (= ["A" "B" "C" "F"] (map letter-grade [95 82 71 55])))
(assert (= :array (type-of (map letter-grade [95 82]))))
(assert (= [95 88] (filter (fn [s] (>= s 80)) [95 72 88 61])))

(assert (= 15 (fold + 0 [1 2 3 4 5])))
(assert (= 6 (apply + [1 2 3])) "apply spreads the arguments")
```

## Sorting

`sort` and `sort-with` keep the collection's type. `sort-by` returns an
`@array` for an immutable array (#1241).

```lisp
(assert (= [1 1 3 4 5] (sort [3 1 4 1 5])))
(assert (= (list "a" "bb" "ccc") (sort-by length (list "bb" "a" "ccc"))))
(assert (= [3 2 1] (sort-with (fn [a b] (compare b a)) [3 1 2])))
```

## Composition and threading

```lisp
# compose chains functions right to left.
(def shout (compose string/upcase (fn [s] (string s "!"))))
(assert (= "HELLO!" (shout "hello")))

# -> threads as the first argument.
(assert (= 30 (-> 5 (+ 10) (* 2))))

# ->> threads as the last argument.
(assert (= [1 9 25]
           (->> [1 2 3 4 5]
             (filter odd?)
             (map (fn [x] (* x x))))))
```

## Tail call optimization

Tail calls run in constant stack space.

```lisp
(defn sum-to [n acc]
  (if (= n 0)
    acc
    (sum-to (- n 1) (+ acc n))))

(assert (= 5000050000 (sum-to 100000 0)) "no stack overflow")
```

## Recursion depth

A non-tail call waits in the fiber, not on the thread's native stack. A
recursion can therefore go as deep as memory allows, up to the depth cap.

```lisp
(defn count-down [n]
  (if (= n 0) 0 (+ n (count-down (- n 1)))))

(assert (= (count-down 100000) 5000050000) "non-tail recursion 100,000 deep")
```

The cap is 10,000,000 calls in progress on one fiber, and
`(vm/config-set :max-depth n)` changes it ([config.md](config.md)). A call past
the cap halts the program with `:stack-overflow`. The halt is not an error, so
`protect` does not catch it.

A recursion that passes through a primitive still uses the native stack at
each level: a trait method that calls the primitive it implements, or `eval`
of a form that calls `eval`. When that stack runs low, the program halts with
`:stack-overflow` too.

---

## See also

- [named-args.md](named-args.md) — &named, &keys, &opt, default
- [destructuring.md](destructuring.md) — unpacking in function params
- [signals](signals/index.md) — how signals affect function contracts
- [control.md](control.md) — conditionals and loops
