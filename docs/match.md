# Pattern Matching

`match` dispatches on the structure and value of data. The compiler
**errors** on non-exhaustive patterns — every `match` must end with a
wildcard (`_`) or a variable pattern to cover all cases. Any unbound
symbol works as a wildcard.

## Basic patterns

```lisp
(defn describe [val]
  (match val
    (0      "zero")
    (1      "one")
    (_      "other")))

(describe 0)               # => "zero"
(describe 1)               # => "one"
(describe 42)              # => "other"
```

## Binding patterns

Unbound symbols in patterns bind the matched value.

```lisp
(defn first-or-default [lst fallback]
  (match lst
    ((x & _) x)
    (_       fallback)))

(first-or-default (list 10 20) :none)  # => 10
(first-or-default (list) :none)        # => :none
```

## Array and struct patterns

```lisp
(defn point-type [p]
  (match p
    ([0 0]    :origin)
    ([x 0]    :x-axis)
    ([0 y]    :y-axis)
    ([x y]    :general)
    (_        :unknown)))

(point-type [0 0])         # => :origin
(point-type [5 0])         # => :x-axis
(point-type [3 4])         # => :general
```

Struct patterns match by key, with literal values for dispatch:

```lisp
(defn area [shape]
  (match shape
    ({:type :circle :radius r}  (* 3.14159 r r))
    ({:type :square :side s}    (* s s))
    (_                          0)))

(area {:type :circle :radius 5})   # => 78.53975
(area {:type :square :side 7})     # => 49
```

## Nested patterns

Patterns compose to any depth:

```lisp
(defn db-host [config]
  (match config
    ({:db {:host h}} h)
    (_               "unknown")))

(db-host {:db {:host "pg.local"}})   # => "pg.local"
(db-host {:nodb true})               # => "unknown"
```

## Guards

`when` clauses add conditions to patterns:

```lisp
(defn classify [n]
  (match n
    (x (when (> x 0)) :positive)
    (0                 :zero)
    (x                 :negative)))

(classify 5)               # => :positive
(classify 0)               # => :zero
(classify -3)              # => :negative
```

---

## See also

- [destructuring.md](destructuring.md) — destructuring in bindings
- [control.md](control.md) — if, cond, case
- [errors.md](errors.md) — error handling
