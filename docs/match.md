# Pattern Matching

`match` dispatches on the structure and value of data. Arms are tried
top to bottom; the first pattern that matches (and whose guard, if any,
passes) selects the body. If no arm matches, a runtime `:match-error`
is raised carrying the unmatched value. A catch-all final arm — a
wildcard (`_`) or a variable pattern — is idiomatic when a fallback
makes sense, but it is not required: omitting it means "no other value
can reach this match", and the runtime error enforces that claim.

The compiler **errors** on unreachable arms — an arm that earlier arms
already cover (a duplicated literal, or anything after a guardless
catch-all) is rejected at compile time.

## Basic patterns

Literal values (numbers, keywords, strings, booleans) match by equality:

```lisp
(defn describe [val]
  (match val
    0      "zero"
    1      "one"
    _      "other"))

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
    (x & _) x
    _       fallback))

(first-or-default (list 10 20) :none)  # => 10
(first-or-default (list) :none)        # => :none
```

**Important:** a bare symbol always binds, never compares:

```lisp
(def x 42)
(match 99
  x x)      # x binds to 99, body returns 99 — NOT a comparison with 42
```

A bare symbol is a catch-all, so no arm may follow it — the compiler
rejects unreachable arms.

To dispatch against a variable's value, use `case` or a guard:

```
(def quit-code 0x100)

# case — evaluates keys, compares with =
(case etype
  quit-code :quit
  :other)

# match — guard compares explicitly
(match etype
  t when (= t quit-code) :quit
  _ :other)
```

## Or-patterns

`(or ...)` in a pattern matches any of the listed alternatives:

```lisp
(defn parity [n]
  (match n
    (or 1 3 5 7 9) :odd
    (or 0 2 4 6 8) :even
    _              :out-of-range))

(parity 3)                 # => :odd
(parity 4)                 # => :even
(parity 42)                # => :out-of-range
```

## Array and struct patterns

```lisp
(defn point-type [p]
  (match p
    [0 0]    :origin
    [x 0]    :x-axis
    [0 y]    :y-axis
    [x y]    :general
    _        :unknown))

(point-type [0 0])         # => :origin
(point-type [5 0])         # => :x-axis
(point-type [3 4])         # => :general
```

Struct patterns match by key, with literal values for dispatch:

```lisp
(defn area [shape]
  (match shape
    {:type :circle :radius r}  (* 3.14159 r r)
    {:type :square :side s}    (* s s)
    _                          0))

(area {:type :circle :radius 5})   # => 78.53975
(area {:type :square :side 7})     # => 49
```

## Nested patterns

Patterns compose to any depth:

```lisp
(defn db-host [config]
  (match config
    {:db {:host h}} h
    _               "unknown"))

(db-host {:db {:host "pg.local"}})   # => "pg.local"
(db-host {:nodb true})               # => "unknown"
```

## Or-patterns

`(or p1 p2 ...)` matches if any alternative matches. All alternatives
must bind the same set of variables (or none at all).

```lisp
(defn classify-suit [suit]
  (match suit
    (or :hearts :diamonds) :red
    (or :clubs :spades)    :black
    _                      :unknown))

(classify-suit :hearts)    # => :red
(classify-suit :spades)    # => :black
```

Or-patterns work with binding patterns — each alternative must bind
the same names:

```lisp
(defn first-element [coll]
  (match coll
    (or [x & _] (x & _))  x
    _                      nil))

(first-element [10 20])        # => 10
(first-element (list 30 40))   # => 30
```

## Guards

`when` inside an arm adds a condition. The syntax is
`(pattern when condition body)` — `when` is a bare keyword
between the pattern and the body, **not** wrapped in parentheses:

```lisp
(defn classify [n]
  (match n
    x when (> x 0) :positive
    0               :zero
    x               :negative))

(classify 5)               # => :positive
(classify 0)               # => :zero
(classify -3)              # => :negative
```

Guards can reference bindings from the pattern:

```lisp
(defn describe-pair [p]
  (match p
    [a b] when (> a b) "descending"
    [a b] when (= a b) "equal"
    [a b]              "ascending"
    _                  "not a pair"))
```

When a guarded arm's pattern is an or-pattern, a failed guard retries
the remaining alternatives — each alternative re-binds and re-tests the
guard before the match moves on to the next arm:

```lisp
# alternative 1 binds x to the head (:a), the guard fails;
# alternative 2 retries with x bound to the tail (5) and passes
(assert (= (match (pair :a 5)
             (or (x . _) (_ . x)) when (= x 5) x
             _ :none) 5))
```

## No matching arm

When no arm matches, `match` raises a `:match-error` carrying the
unmatched value. Catch it with `protect` (or a `try` handler) like any
other error:

```lisp
(def [ok? err] (protect (match 5
                          1 :one
                          2 :two)))

(assert (not ok?))
(assert (= (get err :error) :match-error))
(assert (= (get err :value) 5))
```

A guard that fails on the final arm falls through the same way:

```
(match -1
  x when (> x 0) :positive)   # raises :match-error — guard rejected -1
```

A match that can fail this way is typed as possibly erroring: signal
inference marks it with `:error` unless some guardless arm is
irrefutable (a wildcard or variable). Inside a `(silent!)` function,
use a catch-all arm — a match without one is a compile-time signal
violation.

## Unreachable arms

An arm that earlier arms already cover can never match — the compiler
rejects it:

```
(match n
  _ :anything
  1 :one)        # compile error: unreachable match arm 2

(match n
  1 :one
  1 :uno         # compile error: unreachable match arm 2
  _ :other)
```

Guarded arms never make later arms unreachable — the guard may fail at
runtime, so the compiler assumes both outcomes are possible:

```lisp
(assert (= (match 1
             x when false :never
             1            :one) :one))
```

The same analysis applies *inside* or-patterns, at any nesting depth:
each alternative must match something that earlier arms and earlier
alternatives do not. A dead alternative is a compile error:

```
(match n
  1        :one
  (or 1 2) :other)   # compile error: alternative 1 of the or-pattern
                     # is unreachable — arm 1 already matches 1

(match p
  (or (x . _) (_ . x)) x)   # compile error: every pair matches the
                            # first alternative, so the second is dead
```

On a **guarded** arm, earlier alternatives of the same or-pattern never
make later ones dead: a failed guard retries the remaining alternatives
(see Guards above), so `(or (x . _) (_ . x)) when (= x 5)` is legal —
the second alternative is reachable through guard fallthrough. Coverage
by earlier *arms* still applies to guarded arms as usual.

## match vs case vs cond

| | `match` | `case` | `cond` |
|---|---------|--------|--------|
| **Dispatch** | structural patterns | equality (`=`) against evaluated expressions | arbitrary test expressions |
| **Variables** | bare symbols **bind** | keys are **evaluated** and compared | full expressions |
| **No match** | runtime `:match-error`; unreachable arms are compile errors | falls through to default | falls through |
| **Use when** | dispatching on shape, type, or literal values | dispatching against runtime values | multi-branch boolean logic |

```
# match: literal keyword patterns
(match event-type
  :quit      (handle-quit)
  :key-down  (handle-key ev)
  _          nil)

# case: dispatch against variables holding event codes
(case raw-event-code
  event-quit      (handle-quit)
  event-key-down  (handle-key ev)
  (handle-unknown))

# cond: arbitrary boolean conditions
(cond
  (> x 10) :large
  (> x 0)  :small
  (= x 0)  :zero
  :negative)
```

When `cond` branches are all testing the same expression against literal
values, `match` is more concise: it rejects unreachable arms at compile
time and raises a `:match-error` at runtime when no arm covers the value,
instead of silently falling through. See [control.md](control.md) for
`cond` and `case`.

---

## See also

- [destructuring.md](destructuring.md) — destructuring in bindings
- [destructuring-advanced.md](destructuring-advanced.md) — rest, nesting, match integration
- [control.md](control.md) — if, cond, case, when, unless
- [errors.md](errors.md) — error handling
