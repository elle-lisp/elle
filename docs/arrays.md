# Arrays

<!-- audited: 2026-09-23 -->

Arrays are indexed sequences. Bare `[...]` is immutable; `@[...]` is mutable.

## Literals

```lisp
(assert (= :array (type-of [1 2 3])))
(assert (= :@array (type-of @[1 2 3])))
(assert (= :array (type-of (array 1 2 3))) "the functional constructor is immutable")

# Equality compares elements and ignores mutability.
(assert (= [1 2 3] @[1 2 3]))
```

## Access

```lisp
(assert (= 10 (get [10 20 30] 0)))
(assert (= 30 (get [10 20 30] -1)) "a negative index counts from the end")
(assert (= 3 (length [1 2 3])))
(assert (empty? []))
(assert (= [20 30] (slice [10 20 30 40] 1 3)))

# An array is a function of its index.
(def v [10 20 30])
(assert (= 10 (v 0)))
(assert (= 30 (v -1)))
```

## Immutable operations

`put` and `push` on an immutable array return a new array; the original is
unchanged.

```lisp
(def arr [10 20 30])
(assert (= [99 20 30] (put arr 0 99)))
(assert (= [10 20 30 40] (push arr 40)))
(assert (= [10 20 30] arr) "unchanged")
(assert (= [1 2 3 4] (concat [1 2] [3 4])))
```

`concat` takes any number of collections of one family and joins them in one
linear pass. Concatenating N arrays, strings or byte vectors costs O(total
length) time. A mutable first argument is extended in place and returned.

## Mutable @array operations

`put`, `push`, and `pop` mutate in place. `put` and `push` return the
mutated array; `pop` returns the removed element.

```lisp
(def buf @[1 2 3])
(assert (%identical? buf (push buf 4)) "push returns buf itself")
(assert (= 4 (pop buf)))
(assert (%identical? buf (put buf 0 99)))
(assert (= @[99 2 3] buf))
(assert (= 3 (length buf)))
```

## Higher-order functions

`map` and `filter` keep the collection's type: an array gives an array, an
`@array` an `@array`, a list a list.

```lisp
(assert (= [1 4 9 16] (map (fn [x] (* x x)) [1 2 3 4])))
(assert (= :array (type-of (map inc [1 2]))))
(assert (= :@array (type-of (map inc @[1 2]))))
(assert (= :list (type-of (map inc (list 1 2)))))
(assert (= [1 3 5] (filter odd? [1 2 3 4 5])))
(assert (= :array (type-of (filter odd? [1 2 3]))))
(assert (= 15 (fold + 0 [1 2 3 4 5])))
```

## Sorting

`sort` and `sort-with` keep the collection's type. `sort` on an `@array` sorts
it in place.

```lisp
(assert (= [1 1 3 4 5] (sort [3 1 4 1 5])))
(assert (= :array (type-of (sort [2 1]))))
(def unsorted @[3 1 2])
(sort unsorted)
(assert (= [1 2 3] unsorted) "sorted in place")
(assert (= (list "a" "bb" "ccc") (sort-by length (list "bb" "a" "ccc"))))
```

`sort-by` returns an `@array` for an immutable array (#1241).

## Type conversion

```lisp
(assert (= [1 2 3] (->array (list 1 2 3))))
(assert (= (list 1 2 3) (->list [1 2 3])))
(assert (= :array (type-of (freeze @[1 2]))))
(assert (= :@array (type-of (thaw [1 2]))))
```

---

## See also

- [structs.md](structs.md) — struct and @struct operations
- [sets.md](sets.md) — set operations
- [types.md](types.md) — mutability and type predicates
- [destructuring.md](destructuring.md) — array destructuring patterns
