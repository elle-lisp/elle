# Empty list vs nil

`nil` and `()` are two completely different things. This is not arbitrary.

## What they are

`()` is an empty list — a container that happens to hold zero elements. It is
a real value, like `0` is a real integer or `""` is a real string.

```lisp
(pop '(x))                  # => () — take x out of (x), you get ()
(not= '() nil)               # => true — () is not "absence of value"
```

`nil` is the absence of a value. It means "nothing here", "not found", "no
result". It is falsy.

```lisp
(get {:a 1} :b)              # => nil — key not present
(get {:a 1} :b :missing)     # => :missing — nil means "fall through to default"
```

## Why this matters: truthiness

Since `()` is a real value (an empty container, not "nothing"), it is truthy.
Only `nil` and `false` are falsy.

```lisp
(if '()   :yes :no)          # => :yes — empty list is a value
(if nil   :yes :no)          # => :no  — nil is absence
(if false :yes :no)          # => :no  — false is false
```

This lets you distinguish "the function returned an empty result" from "the
function returned nothing":

```lisp
(defn find-all [pred coll]
  (filter pred coll))

(if (find-all odd? [2 4 6])    # => () — truthy, meaning: "ran, found nothing"
  :got-results
  :no-results)                 # => :got-results — correct!

(if (find-all odd? nil)         # => error — nil isn't a collection
  ...)
```

## Why this matters: pattern matching

Because `()` and `nil` are distinct, `match` can dispatch on each separately:

```lisp
(defn describe [x]
  (match x
    ()       "empty list"
    nil      "nothing"
    (a)      (string "one element: " a)
    (a b)    (string "two elements: " a " and " b)
    _        "something else"))

(describe '())     # => "empty list"
(describe nil)     # => "nothing"
(describe '(42))   # => "one element: 42"
```

If `()` and `nil` were the same, you could not distinguish "empty result"
from "no result" in a match arm.

## Why this matters: list termination

Lists are linked lists of cons cells. The tail of `(1 2 3)` is `(2 3)`.
The tail of `(3)` is `()`. This means:

```lisp
(rest '(1 2 3))    # => (2 3)
(rest '(3))        # => ()
(rest '())         # => ()
```

Lists terminate with `()`, not `nil`. This is why `(nil? (rest '(3)))`
returns `false` — use `empty?` instead:

```lisp
(defn my-length [lst]
  (if (empty? lst)
    0
    (+ 1 (my-length (rest lst)))))

(my-length '(1 2 3))    # => 3
```

Using `nil?` here instead of `empty?` causes **infinite recursion**, because
`(rest '(3))` returns `()`, which is truthy, so the base case never triggers.

## Implementation: dedicated bits in the Value type

`()` and `nil` each have their own tag in Elle's 16-byte `Value` tagged union.
This is intentional — it means the VM can distinguish them in a single tag
check, with no overhead.

A `Value` is `(tag: u64, payload: u64)`. The tag for `()` is distinct from the
tag for `nil`. Both are immediate values — no heap allocation, no pointer
chase. `empty?` is a tag comparison. `nil?` is a tag comparison. They check
different tags.

---

## See also

- [types.md](types.md) — full type system, truthiness rules, predicate table
- [syntax.md](syntax.md) — `()` and `nil` literal syntax
- [destructuring.md](destructuring.md) — rest patterns produce `()`, not `nil`
