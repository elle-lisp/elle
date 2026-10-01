# Loops

<!-- audited: 2026-09-30 -->

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

`(each name in coll body...)` binds `name` to each element of `coll` in
turn and runs `body`. The `in` is optional. `each` returns `nil`, or the
value a `break` carries.

```lisp
(var total 0)
(each x in [10 20 30]
  (assign total (+ total x)))
(assert (= total 60))
(assert (nil? (each x [1 2 3] x)))
```

The collection decides what an element is:

| Collection | Element |
|------------|---------|
| list, array, `@array` | each item, in order |
| string, `@string` | each grapheme cluster, as a string |
| bytes, `@bytes` | each byte, as an integer |
| set, `@set` | each member, in no promised order |
| struct, `@struct` | each `[key value]` array, in the order `pairs` gives |
| fiber | each value the fiber yields |
| a value whose traits carry `:iter` | each value its iterator yields |

```lisp
(def graphemes @[])
(each g in "a👋🏽b" (push graphemes g))
(assert (= (freeze graphemes) ["a" "👋🏽" "b"]))

(def entries @[])
(each [k v] in {:a 1 :b 2} (push entries [k v]))
(assert (= (freeze entries) (->array (pairs {:a 1 :b 2}))))
```

[structs.md](structs.md) holds the key order, and [traits.md](traits.md)
the `:iter` protocol.

`each` resumes a fiber once per pass. Every value the fiber yields is an
element, `nil` and `false` included. The loop ends when the fiber completes,
and the value the fiber returns is not an element. An error inside the fiber
propagates out of `each`, also when the fiber's mask catches errors. A fiber
that has already completed raises, as `fiber/resume` does.

```lisp
(def gen (fiber/new (fn [] (yield 1) (yield nil) (yield false) :done)
                    |:yield|))
(def yielded @[])
(each v in gen (push yielded v))
(assert (= (freeze yielded) [1 nil false]))
```

Any other value raises a `:type-error`, and `nil` is no exception.

```lisp
(let [[ok? err] (protect (each x nil x))]
  (assert (not ok?))
  (assert (= (err :error) :type-error)))
```

## Early exit

A `break` with no label leaves the `each`, and its value becomes the value
of the `each`:

```lisp
(assert (= (each x [2 4 6 8 10] (when (> x 7) (break x))) 8))
```

Use `block` + `break` to leave a form around the `each`:

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
- [traits.md](traits.md) — the `:iter` protocol a custom collection carries
- [signals/primitives.md](signals/primitives.md) — fibers as generators
