# Functions

## fn — anonymous functions

`fn` creates a closure. Brackets delimit the parameter list. Parameters
are immutable by default; prefix with `@` to allow mutation via `assign`.

```lisp
(def double (fn [x] (* x 2)))
(double 21)                # => 42

# mutable parameter
(def bump (fn [@n] (assign n (+ n 1)) n))
(bump 10)                  # => 11
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

(letter-grade 95)          # => "A"
(letter-grade 55)          # => "F"
(doc letter-grade)         # => "Convert a numeric score to a letter grade."
```

## Variadic functions

`&` collects remaining arguments into a list.

```lisp
(defn sum [& nums]
  (fold + 0 nums))

(sum 1 2 3 4)              # => 10
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
(counter)                  # => 1
(counter)                  # => 2
(counter)                  # => 3
```

## Higher-order functions

```lisp
(map letter-grade [95 82 71 55])
# => ("A" "B" "C" "F")    — map always returns a list

(filter (fn [s] (>= s 80)) [95 72 88 61])
# => (95 88)               — filter always returns a list

(fold + 0 [1 2 3 4 5])    # => 15
(apply + [1 2 3])          # => 6 (spread args)
```

`map` and `filter` return lists even when given arrays. Use
`->array` or `stream/into-array` to get arrays back.

## Sorting

```lisp
(sort [3 1 4 1 5])                    # => (1 1 3 4 5)
(sort-by length ["bb" "a" "ccc"])     # => ("a" "bb" "ccc")
(sort-with (fn [a b] (compare b a)) [3 1 2])  # => (3 2 1)
```

## Composition and threading

```lisp
# compose chains functions right-to-left
(def shout (compose string/upcase (fn [s] (string s "!"))))
(shout "hello")            # => "HELLO!"

# -> threads as first argument
(-> 5 (+ 10) (* 2))       # => 30

# ->> threads as last argument
(->> [1 2 3 4 5]
  (filter odd?)
  (map (fn [x] (* x x))))  # => (1 9 25)
```

## Tail call optimization

Tail calls are guaranteed to run in constant stack space.

```lisp
(defn sum-to [n acc]
  (if (= n 0)
    acc
    (sum-to (- n 1) (+ acc n))))

(sum-to 10000 0)           # => 50005000 — no stack overflow
```

---

## See also

- [named-args.md](named-args.md) — &named, &keys, &opt, default
- [destructuring.md](destructuring.md) — unpacking in function params
- [signals](signals/index.md) — how signals affect function contracts
- [control.md](control.md) — conditionals and loops
