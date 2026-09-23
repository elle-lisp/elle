# Pattern Matching

<!-- audited: 2026-09-23 -->

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

(assert (= (describe 0) "zero"))
(assert (= (describe 1) "one"))
(assert (= (describe 42) "other"))
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

(assert (= (first-or-default (list 10 20) :none) 10))
(assert (= (first-or-default (list) :none) :none))
```

**Important:** a bare symbol always binds, never compares:

```lisp
(def x 42)
(assert (= (match 99
             x x)       # x binds to 99 — NOT a comparison with 42
           99))
```

A bare symbol is a catch-all, so no arm may follow it — the compiler
rejects unreachable arms.

To dispatch against a variable's value, use `case` or a guard:

```lisp
(def quit-code 0x100)

(defn by-case [etype]
  (case etype               # case evaluates its keys and compares with =
    quit-code :quit
    :other))

(defn by-guard [etype]
  (match etype
    t when (= t quit-code) :quit   # a guard compares explicitly
    _ :other))

(assert (= (by-case 256) :quit))
(assert (= (by-guard 256) :quit))
(assert (= (by-guard 1) :other))
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

(assert (= (point-type [0 0]) :origin))
(assert (= (point-type [5 0]) :x-axis))
(assert (= (point-type [3 4]) :general))
```

Struct patterns match by key, with literal values for dispatch:

```lisp
(defn area [shape]
  (match shape
    {:type :circle :radius r}  (* 3.14159 r r)
    {:type :square :side s}    (* s s)
    _                          0))

(assert (= (area {:type :circle :radius 5}) 78.53975))
(assert (= (area {:type :square :side 7}) 49))
```

## Nested patterns

Patterns compose to any depth:

```lisp
(defn db-host [config]
  (match config
    {:db {:host h}} h
    _               "unknown"))

(assert (= (db-host {:db {:host "pg.local"}}) "pg.local"))
(assert (= (db-host {:nodb true}) "unknown"))
```

## Or-patterns

`(or p1 p2 ...)` matches if any alternative matches. All alternatives
must bind the same set of variables (or none at all).

```lisp
(defn parity [n]
  (match n
    (or 1 3 5 7 9) :odd
    (or 0 2 4 6 8) :even
    _              :out-of-range))

(assert (= (parity 3) :odd))
(assert (= (parity 4) :even))
(assert (= (parity 42) :out-of-range))

(defn classify-suit [suit]
  (match suit
    (or :hearts :diamonds) :red
    (or :clubs :spades)    :black
    _                      :unknown))

(assert (= (classify-suit :hearts) :red))
(assert (= (classify-suit :spades) :black))
```

Or-patterns work with binding patterns — each alternative must bind
the same names:

```lisp
(defn first-element [coll]
  (match coll
    (or [x & _] (x & _))  x
    _                      nil))

(assert (= (first-element [10 20]) 10))
(assert (= (first-element (list 30 40)) 30))
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

(assert (= (classify 5) :positive))
(assert (= (classify 0) :zero))
(assert (= (classify -3) :negative))
```

Guards can reference bindings from the pattern:

```lisp
(defn describe-pair [p]
  (match p
    [a b] when (> a b) "descending"
    [a b] when (= a b) "equal"
    [a b]              "ascending"
    _                  "not a pair"))

(assert (= (describe-pair [2 1]) "descending"))
(assert (= (describe-pair [1 1]) "equal"))
(assert (= (describe-pair :x) "not a pair"))
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

```lisp
(def [positive? rejected] (protect (match -1
                                     x when (> x 0) :positive)))
(assert (not positive?))
(assert (= (get rejected :error) :match-error))
```

A match that can fail this way is typed as possibly erroring: signal
inference marks it with `:error` unless some guardless arm is
irrefutable (a wildcard or variable). Inside a `(silent!)` function,
use a catch-all arm — a match without one is a compile-time signal
violation.

```lisp
(defn compiles? [src]
  (first (protect (compile/whole-module src "<doc>"))))

(assert (not (compiles? "(defn f [n] (silent!) (match n 1 :one 2 :two))")))
(assert (compiles? "(defn f [n] (silent!) (match n 1 :one _ :two))"))
```

## Unreachable arms

An arm that earlier arms already cover can never match — the compiler
rejects it with "unreachable match arm 2":

```lisp
(assert (not (compiles? "(defn f [n] (match n _ :anything 1 :one))")))
(assert (not (compiles? "(defn f [n] (match n 1 :one 1 :uno _ :other))")))
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
alternatives do not. A dead alternative is a compile error. Below, arm 1
already matches `1`, and in the second source every pair matches the first
alternative, so the second is dead:

```lisp
(assert (not (compiles? "(defn f [n] (match n 1 :one (or 1 2) :other))")))
(assert (not (compiles? "(defn f [p] (match p (or (x . _) (_ . x)) x))")))
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
| **No match** | runtime `:match-error`; unreachable arms are compile errors | the default, else `nil` | the default, else `nil` |
| **Use when** | dispatching on shape, type, or literal values | dispatching against runtime values | multi-branch boolean logic |

```lisp
(def event-quit 12)
(def event-key-down 768)

(defn by-type [event-type]
  (match event-type          # match: literal keyword patterns
    :quit      :bye
    :key-down  :typed
    _          nil))

(defn by-code [raw-event-code]
  (case raw-event-code       # case: keys are variables holding event codes
    event-quit      :bye
    event-key-down  :typed
    :unknown))

(defn size [x]
  (cond                      # cond: arbitrary boolean conditions
    (> x 10) :large
    (> x 0)  :small
    (= x 0)  :zero
    :negative))

(assert (= (by-type :quit) :bye))
(assert (= (by-code 768) :typed))
(assert (= (size -4) :negative))
(assert (nil? (case 5 1 :a 2 :b)))
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
