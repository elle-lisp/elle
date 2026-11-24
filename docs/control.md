# Control Flow

<!-- audited: 2026-11-24 -->

Elle control flow forms are expressions — they return values. Only `nil` and
`false` are falsy; everything else is truthy.

## if

`(if test then else)` — else is optional and defaults to `nil`.

```lisp
(if true :yes :no)         # => :yes
(if false :yes :no)        # => :no
(if nil :yes :no)          # => :no
(if false :yes)            # => nil (else defaults to nil)

# 0, empty string, empty list are all truthy
(if 0 :yes :no)            # => :yes
(if "" :yes :no)           # => :yes
(if (list) :yes :no)       # => :yes
```

## cond

Multi-branch conditional. Evaluates tests in order, returns the body of the
first truthy one. For structural dispatch on data shape, see
[match.md](match.md).

```lisp
(defn classify [x]
  (cond
    (> x 10) :large
    (> x 0)  :small
    (= x 0)  :zero
    :negative))

(classify 42)              # => :large
(classify 3)               # => :small
(classify 0)               # => :zero
(classify -5)              # => :negative
```

`cond` evaluates arbitrary test expressions. When you need to dispatch on
the *structure* or *literal value* of a single expression, `match` is more
concise. It rejects unreachable arms at compile time, and signals a
`:match-error` at run time when no arm matches.

## case

Equality dispatch on a value. Flat pairs of `value body`, with an optional
default at the end.

```lisp
(defn describe [op]
  (case op
    :add "addition"
    :sub "subtraction"
    :mul "multiplication"
    "unknown"))

(describe :add)            # => "addition"
(describe :nope)           # => "unknown"
```

## when / unless

One-armed conditionals. Return `nil` when the test does not fire.

```lisp
(when true :yes)           # => :yes
(when false :yes)          # => nil
(unless false :yes)        # => :yes
(unless true :yes)         # => nil
```

## when-let / if-let

Bind a value and branch on its truthiness.

```lisp
(defn safe-div [a b]
  (if (= b 0) nil (/ a b)))

# if-let: two branches
(if-let [q (safe-div 10 2)]
  (+ q 1)
  :fail)                   # => 6

(if-let [q (safe-div 10 0)]
  (+ q 1)
  :fail)                   # => :fail

# when-let: one branch, nil otherwise
(when-let [q (safe-div 15 3)]
  (* q 10))                # => 50
```

Several bindings are tested in order, and the first falsy value chooses the
else form; the expressions after it do not run. Each binding's expression sees
the names before it, and both forms see every name. In the else form, a name
holds the value its expression gave, and a name whose expression did not run
is nil. A destructuring pattern whose value is falsy or never computed binds
each of its names to nil.

```lisp
(assert (= (if-let [a 1 b (+ a 1)] [a b] :else) [1 2]) "b's expression sees a")
(assert (= (if-let [a nil b (error :ran)] :then :else) :else)
        "a falsy value stops the bindings after it")
(assert (= (if-let [a nil b 2] :then [a b]) [nil nil]) "the else form sees every name")
(assert (= (if-let [a 1 b false] :then [a b]) [1 false]))
(assert (= (if-let [[a b] [1 2] c nil] :then [a b c]) [1 2 nil]) "a pattern that ran")
(assert (= (if-let [x nil {:k k} {:k 1}] :then k) nil) "a pattern that did not run")
```

A file of epoch 18 or older expands `if-let` the same way, so its else form
reads the names the `if-let` binds, not the names around it.

## begin

Evaluates expressions in sequence, sharing the surrounding scope. Returns
the last value. Used in macro expansions. `begin` does not create a new scope,
so a `def` inside a `begin` that stands directly in a body binds in that body
([bindings](bindings.md)).

```lisp
(var x 0)
(def result
  (begin
    (assign x 1)
    (assign x (+ x 1))
    (* x 10)))
result                     # => 20
x                          # => 2 (begin shared the scope)
```

## block

Creates a new lexical scope. Supports named early exit via `break`.

```lisp
# returns last value
(block :b (+ 1 2) (* 3 4))  # => 12

# break exits early with a value
(block :search
  (each x [10 20 30 40]
    (when (> x 25)
      (break :search x)))
  nil)                     # => 30

# nested blocks: break targets a specific label
(block :outer
  (block :inner
    (break :outer :escaped))
  :unreachable)            # => :escaped
```

`break` does NOT cross function boundaries — validated at compile time.

## Loops

[loops.md](loops.md) covers `while`, `forever`, `repeat` and `each`.

---

## See also

- [match.md](match.md) — pattern matching
- [errors.md](errors.md) — error handling and recovery
- [functions.md](functions.md) — fn, defn, closures
