# Traits

Traits attach metadata to values. Any heap-allocated value can carry a
trait table (an immutable struct).

## Attaching traits

```lisp
(def v (with-traits [1 2 3] {:type :point :dim 2}))
(traits v)                 # => {:type :point :dim 2}
(traits [1 2 3])           # => nil (no traits attached)
```

## Traits are invisible to equality

Traits do not affect structural equality, ordering, or hashing:

```lisp
(def plain [1 2 3])
(def annotated (with-traits [1 2 3] {:type :point}))
(= plain annotated)        # => true
```

## Dispatch on traits

```lisp
(defn describe [val]
  (match (traits val)
    {:type :point}  "a point"
    {:type :color}  "a color"
    _               "unknown"))

(describe (with-traits [255 0 0] {:type :color}))  # => "a color"
(describe (with-traits [1 2] {:type :point}))      # => "a point"
(describe [1 2 3])                                  # => "unknown"
```

---

## See also

- [types.md](types.md) — type system
- [structs.md](structs.md) — struct operations (trait tables are structs)
- [match.md](match.md) — pattern matching for dispatch
