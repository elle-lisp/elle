# Sets

Sets are unordered collections of unique values. `|...|` is immutable;
`@|...|` is mutable.

## Literals

```lisp
|1 2 3|              # immutable set
@|1 2 3|             # mutable @set
|:a :b :c|           # keywords work too
```

## Membership and size

```lisp
(def s |1 2 3|)
(contains? s 2)      # => true
(contains? s 9)      # => false
(length s)           # => 3
(empty? ||)          # => true

# callable set syntax — sets are functions of their elements
(|:a :b :c| :c)     # => true
(|:a :b :c| :d)     # => false
(s 2)                # => true
```

## Set operations

```lisp
(def a |1 2 3|)
(def b |2 3 4|)

(union a b)          # => |1 2 3 4|
(intersection a b)   # => |2 3|
(difference a b)     # => |1|
```

## Mutable @sets

```lisp
(def ms @|1 2 3|)
(add ms 4)           # mutates; ms is now @|1 2 3 4|
(del ms 1)           # mutates; ms is now @|2 3 4|
```

## As signal masks

Set literals are the preferred syntax for fiber signal masks:

```lisp
# fiber that catches yield and io signals
(fiber/new |:yield :io| (fn [] (yield 42)))
```

---

## See also

- [arrays.md](arrays.md) — array operations
- [structs.md](structs.md) — struct operations
- [types.md](types.md) — type predicates and mutability
- [signals](signals/index.md) — signal masks use set literals
