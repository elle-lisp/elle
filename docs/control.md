# Control Flow

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
first truthy one.

```lisp
(defn classify [x]
  (cond
    ((> x 10) :large)
    ((> x 0)  :small)
    ((= x 0)  :zero)
    (true     :negative)))

(classify 42)              # => :large
(classify 3)               # => :small
(classify 0)               # => :zero
(classify -5)              # => :negative
```

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
(if-let ([q (safe-div 10 2)])
  (+ q 1)
  :fail)                   # => 6

(if-let ([q (safe-div 10 0)])
  (+ q 1)
  :fail)                   # => :fail

# when-let: one branch, nil otherwise
(when-let ([q (safe-div 15 3)])
  (* q 10))                # => 50
```

## begin

Evaluates expressions in sequence, sharing the surrounding scope. Returns
the last value. Used in macro expansions; `begin` does not create a new scope.

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

## while

Loops while the test is truthy. Returns `nil` unless you `break` with a value.

```lisp
(var i 0)
(while (< i 5)
  (assign i (+ i 1)))
i                          # => 5

# break :while returns a value
(var k 0)
(while (< k 100)
  (assign k (+ k 1))
  (when (= k 7) (break :while k)))  # => 7
```

## forever

Sugar for `(while true ...)`. Use `break` to exit.

```lisp
(var n 1)
(forever
  (assign n (* n 2))
  (when (> n 100) (break :while n)))  # => 128
```

## repeat

Runs the body N times. Returns `nil`.

```lisp
(var count 0)
(repeat 5 (assign count (+ count 1)))
count                      # => 5
```

## each

Iteration macro. `in` is optional sugar. Works on lists, arrays, and
other sequences.

```lisp
(var total 0)
(each x in [10 20 30]
  (assign total (+ total x)))
total                      # => 60

# early exit via block + break
(block :found
  (each x [1 4 9 16 25]
    (when (> x 10)
      (break :found x)))
  nil)                     # => 16
```

---

## See also

- [match.md](match.md) — pattern matching
- [errors.md](errors.md) — error handling and recovery
- [functions.md](functions.md) — fn, defn, closures
