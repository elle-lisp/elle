# Pattern Matching

`match` dispatches on the structure and value of data. The compiler
**errors** on non-exhaustive patterns — every `match` must end with a
wildcard (`_`) or a variable pattern to cover all cases. Any unbound
symbol works as a wildcard.

## Basic patterns

Literal values (numbers, keywords, strings, booleans) match by equality:

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

Unbound symbols in patterns **bind** the matched value — they do not
compare against variables in scope. Use `case` (see
[control.md](control.md)) for equality dispatch against evaluated
expressions.

```lisp
(defn first-or-default [lst fallback]
  (match lst
    ((x & _) x)
    (_       fallback)))

(first-or-default (list 10 20) :none)  # => 10
(first-or-default (list) :none)        # => :none
```

**Important:** a bare symbol always binds, never compares:

```lisp
(def x 42)
(match 99
  (x x)     # x binds to 99, body returns 99
  (_ :no))  # => 99, NOT :no
```

To dispatch against a variable's value, use `case` or a guard:

```
(def quit-code 0x100)

# case — evaluates keys, compares with =
(case etype
  quit-code :quit
  :other)

# match — guard compares explicitly
(match etype
  (t when (= t quit-code) :quit)
  (_ :other))
```

## Or-patterns

`(or ...)` in a pattern matches any of the listed alternatives:

```lisp
(defn parity [n]
  (match n
    ((or 1 3 5 7 9) :odd)
    ((or 0 2 4 6 8) :even)
    (_              :out-of-range)))

(parity 3)                 # => :odd
(parity 4)                 # => :even
(parity 42)                # => :out-of-range
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

`when` inside an arm adds a condition. The syntax is
`(pattern when condition body)` — `when` is a bare keyword
between the pattern and the body, **not** wrapped in parentheses:

```lisp
(defn classify [n]
  (match n
    (x when (> x 0) :positive)
    (0               :zero)
    (x               :negative)))

(classify 5)               # => :positive
(classify 0)               # => :zero
(classify -3)              # => :negative
```

Guards can reference bindings from the pattern:

```lisp
(defn describe-pair [p]
  (match p
    ([a b] when (> a b) "descending")
    ([a b] when (= a b) "equal")
    ([a b]              "ascending")
    (_                  "not a pair")))
```

## match vs case

| | `match` | `case` |
|---|---------|--------|
| **Patterns** | structural (literals, destructuring, guards) | equality (`=`) against evaluated expressions |
| **Variables** | bare symbols **bind** | keys are **evaluated** and compared |
| **Use when** | dispatching on shape, type, or literal values | dispatching against runtime values (constants, computed keys) |

```
# match: literal keyword patterns
(match event-type
  (:quit      (handle-quit))
  (:key-down  (handle-key ev))
  (_          nil))

# case: dispatch against variables holding event codes
(case raw-event-code
  event-quit      (handle-quit)
  event-key-down  (handle-key ev)
  (handle-unknown))
```

---

## See also

- [destructuring.md](destructuring.md) — destructuring in bindings
- [destructuring-advanced.md](destructuring-advanced.md) — rest, nesting, match integration
- [control.md](control.md) — if, cond, case, when, unless
- [errors.md](errors.md) — error handling
