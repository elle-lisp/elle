# Destructuring — Advanced

Advanced destructuring patterns: rest, wildcard in depth, nesting, and
match integration.

## Rest in list vs array

`& rest` collects remaining elements. The collection type matches the
pattern type:

```lisp
# list pattern → rest is a list
(def (a & tail) (list 1 2 3))
(list? tail)               # => true

# array pattern → rest is an array
(def [b & rest] [10 20 30])
(array? rest)              # => true
```

## Struct remainder

`&` in struct patterns collects unmatched keys:

```lisp
(def {:x x & extra} {:x 1 :y 2 :z 3})
x                          # => 1
extra                      # => {:y 2 :z 3}
```

## Deep nesting

Patterns compose freely across list, array, and struct boundaries:

```lisp
# struct containing array containing values
(def {:point [px py]} {:point [3 4]})
px                         # => 3
py                         # => 4

# list of [name, {metadata}]
(def (name {:role role}) (list "Alice" {:role :admin :id 7}))
name                       # => "Alice"
role                       # => :admin

# three-level: config → db → host
(def {:config {:db {:host h :port p}}}
  {:config {:db {:host "pg.local" :port 5432}}})
h                          # => "pg.local"
p                          # => 5432
```

## In match patterns

Destructuring in `match` enables structural dispatch:

```lisp
(defn area [shape]
  (match shape
    {:type :circle :radius r}  (* r r)
    {:type :square :side s}    (* s s)
    _                          0))

(area {:type :circle :radius 5})   # => 25
(area {:type :square :side 3})     # => 9
```

## Mutable destructuring

`var` with destructuring creates mutable bindings:

```lisp
(var (ma mb) (list 1 2))
(assign ma 100)
ma                         # => 100
```

---

## See also

- [destructuring.md](destructuring.md) — basic destructuring
- [match.md](match.md) — pattern matching
- [bindings.md](bindings.md) — binding forms
