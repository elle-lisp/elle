# proof

<!-- audited: 2026-11-18 -->

A Datalog engine: relations of sorted tuples, and variables that grow by rules until a fixpoint, after datafrog 2.0.1.

```lisp
(def proof ((import "std/proof")))
(def edges (proof:relation [[1 2] [2 3] [3 4]]))
(def by-target (proof:relation (map (fn [[a b]] [b a]) (proof:elements edges))))
(def it (proof:iteration))
(def reach (proof:variable it "reach"))
(proof:insert reach edges)
(while (proof:changed? it)
  # reach(a, c) :- edges(a, b), reach(b, c).
  (proof:from-join reach reach by-target (fn [b c a] [a c])))
(assert (= [[1 2] [1 3] [1 4] [2 3] [2 4] [3 4]]
           (proof:elements (proof:complete reach))))
```

## Tuples and relations

A tuple is any value, and a relation is a fixed set of tuples.
`(proof:relation xs)` builds one from an array or a list, and
`(proof:elements r)` returns its tuples as an array, sorted by `compare` and
with no two that compare 0 ([types.md](../types.md)). So `[1]` and `@[1]` are
one tuple.

A join reads each tuple as a pair `[key value]`, and matches keys that
compare 0. A key or a value may be an array, which is how a rule keys on
two columns.

| Call | Returns |
|------|---------|
| `(proof:merge r1 r2)` | the union of two relations |
| `(proof:join r1 r2 f)` | `(f k v1 v2)` for each `[k v1]` of `r1` and `[k v2]` of `r2` |
| `(proof:antijoin r keys f)` | `(f k v)` for each `[k v]` of `r` whose key is not a tuple of `keys` |
| `(proof:map r f)` | `(f t)` for each tuple `t` of `r` |
| `(proof:leapjoin r leapers f)` | `(f t v)` for each tuple `t` of `r` and each value `v` the leapers agree on |

Each call returns a relation.

## Variables and the fixpoint

An iteration holds variables, and a variable is a set of tuples that only
grows. `(proof:iteration)` makes an iteration, and `(proof:variable it name)`
adds a variable to it. `(proof:insert v r)` adds a relation's tuples to a
variable, and `(proof:extend v xs)` adds an array's.

A tuple added to a variable is new for one round, and then it is stable.
`(proof:changed? it)` ends a round. It returns `true` while some variable
holds new tuples, so a loop over it runs the rules until nothing new
appears. A rule reads only what is new. A join pairs the new tuples of each
input with every tuple of the other, and the other rules read the new tuples
of their variable. So each combination of tuples meets once.

| Call | Adds to the variable `out` |
|------|----------------------------|
| `(proof:from-join out v x f)` | `(f k v1 v2)` for `[k v1]` of the variable `v` and `[k v2]` of `x`, a variable or a relation |
| `(proof:from-antijoin out v keys f)` | `(f k val)` for `[k val]` of `v` whose key is not a tuple of the relation `keys` |
| `(proof:from-map out v f)` | `(f t)` for each tuple of `v` |
| `(proof:from-leapjoin out v leapers f)` | `(f t val)` for each tuple `t` of `v` and each value the leapers agree on |

A join of two relations cannot change between rounds. Compute it once with
`proof:join` and insert the result.

`(proof:complete v)` returns a variable's tuples as a relation, once the loop
has ended. Before that it raises `:argument-error`.

A variable made with `:distinct false` does not remove the tuples it already
holds from the new ones. A cycle of rules ends only when one variable on it
is distinct, and every variable is distinct by default.

## Leapers

A leapjoin extends each source tuple with values from several relations in
one step, and proposes each value from the relation that offers the fewest.

| Leaper | Effect on the values of the tuple `t` |
|--------|---------------------------------------|
| `(proof:extend-with r key)` | proposes, or keeps, each `v` with `[(key t) v]` in `r` |
| `(proof:extend-anti r key)` | removes each `v` with `[(key t) v]` in `r` |
| `(proof:filter-with r pair)` | keeps `t` only when the tuple `(pair t)` is in `r` |
| `(proof:filter-anti r pair)` | keeps `t` only when the tuple `(pair t)` is not in `r` |
| `(proof:prefix-filter p)` | keeps `t` only when `(p t)` is truthy |
| `(proof:value-filter p)` | keeps each `v` for which `(p t v)` is truthy |

The leapers argument is one leaper or an array of them. One of them must
propose values: an `extend-with`, or a `filter-with`, `filter-anti` or
`prefix-filter` that stands alone and proposes `nil`. A leapjoin with no
proposer raises `:argument-error`.

```lisp
(def proof ((import "std/proof")))
(def doubles (proof:relation (map (fn [i] [i (* 2 i)]) (range 10))))
(def it (proof:iteration))
(def v (proof:variable it "v"))
(proof:extend v [[2 2]])
(while (proof:changed? it)
  (proof:from-leapjoin v v (proof:extend-with doubles (fn [[i _]] i))
                       (fn [[i _] j] [i j])))
(assert (= [[2 2] [2 4]] (proof:elements (proof:complete v))))
```
