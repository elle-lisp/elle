# Arrays

Arrays are indexed sequences. Bare `[...]` is immutable; `@[...]` is mutable.

## Literals

```lisp
[1 2 3]              # immutable array
@[1 2 3]             # mutable @array
(array 1 2 3)        # functional constructor (immutable)
```

## Access

```lisp
(get [10 20 30] 0)         # => 10
(get [10 20 30] 1)         # => 20
(get [10 20 30] -1)        # => 30 (negative indexes count from end)
(length [1 2 3])           # => 3
(empty? [])                # => true
(slice [10 20 30 40] 1 3)  # => [20 30]

# callable array syntax — arrays are functions of their index
(def v [10 20 30])
(v 0)                      # => 10
(v -1)                     # => 30
```

## Immutable operations

`put` and `push` on an immutable array return a new array; the original is unchanged.

```lisp
(def arr [10 20 30])
(put arr 0 99)             # => [99 20 30]
(push arr 40)              # => [10 20 30 40]
arr                        # => [10 20 30] (unchanged)
(concat [1 2] [3 4])       # => [1 2 3 4]
```

## Mutable @array operations

`put`, `push`, and `pop` mutate in place. `put` and `push` return the
mutated array; `pop` returns the removed element.

```lisp
(def buf @[1 2 3])
(push buf 4)               # appends, returns buf; buf is now @[1 2 3 4]
(pop buf)                  # => 4; buf is now @[1 2 3]
(put buf 0 99)             # mutates and returns buf; buf is now @[99 2 3]
(length buf)               # => 3
```

## Higher-order functions

`map` and `filter` always return lists, even when given arrays.

```lisp
(map (fn [x] (* x x)) [1 2 3 4])
# => (1 4 9 16)

(filter odd? [1 2 3 4 5])
# => (1 3 5)

(fold + 0 [1 2 3 4 5])    # => 15
```

## Sorting

Sort always returns a list.

```lisp
(sort [3 1 4 1 5])                    # => (1 1 3 4 5)
(sort-by length ["bb" "a" "ccc"])     # => ("a" "bb" "ccc")
```

## Type conversion

```lisp
(->array (list 1 2 3))    # => [1 2 3]
(->list [1 2 3])           # => (1 2 3)
(freeze @[1 2])            # => [1 2]
(thaw [1 2])               # => @[1 2]
```

---

## See also

- [structs.md](structs.md) — struct and @struct operations
- [sets.md](sets.md) — set operations
- [types.md](types.md) — mutability and type predicates
- [destructuring.md](destructuring.md) — array destructuring patterns
