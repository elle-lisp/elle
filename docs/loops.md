# Loops

Elle's loop forms are `while`, `forever`, `repeat`, and `each`. All
support early exit via `break`. See [control.md](control.md) for the
full control flow picture.

## while

Loops while the test is truthy. Returns `nil` unless you `break` with
a value. The implicit block is named `:while`.

```lisp
(var i 0)
(while (< i 5)
  (assign i (+ i 1)))
i                          # => 5
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
(repeat 10 (assign count (+ count 1)))
count                      # => 10
```

## each

Iteration macro. `in` is optional sugar.

```lisp
(var total 0)
(each x in [10 20 30]
  (assign total (+ total x)))
total                      # => 60
```

Works on lists, arrays, and other sequences.

## while-let

Loop while a binding succeeds (value is truthy):

```text
(while-let [[line (port/read-line port)]]
  (println line))
```

## Early exit

Use `block` + `break` for search patterns:

```lisp
(block :found
  (each x [2 4 6 8 10]
    (when (> x 7)
      (break :found x)))
  nil)                     # => 8
```

## Iterate with index

```lisp
(var i 0)
(each x in [:a :b :c]
  (println i " " x)
  (assign i (+ i 1)))
```

---

## See also

- [control.md](control.md) — full control flow reference
- [arrays.md](arrays.md) — array iteration
- [coroutines.md](coroutines.md) — lazy generators
