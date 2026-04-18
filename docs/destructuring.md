# Destructuring

Destructuring unpacks collections into bindings. It works in `def`, `var`,
`let`, `let*`, `fn`, `defn`, and `match`.

Destructuring is strict — missing elements or keys signal an error.
Extra elements are silently ignored.

## List patterns

```lisp
(def (a b c) (list 1 2 3))
a     # => 1
c     # => 3

# & rest collects remaining elements into a list
(def (head & tail) (list 1 2 3 4))
head  # => 1
tail  # => (2 3)

# all consumed: rest is empty list (not nil!)
(def (p q & rest) (list 10 20))
(empty? rest)    # => true
(nil? rest)      # => false
```

## Array patterns

```lisp
(def [x y] [10 20])
x     # => 10
y     # => 20

# & rest collects into an array (not a list)
(def [first & rest] [10 20 30])
first            # => 10
(array? rest)    # => true
(get rest 0)     # => 20
```

## Struct patterns

```lisp
(def {:name n :age a} {:name "Alice" :age 30})
n     # => "Alice"
a     # => 30

# & collects unmatched keys into a struct
(def {:a va & more} {:a 1 :b 2 :c 3})
va    # => 1
more  # => {:b 2 :c 3}
```

## Wildcard

`_` discards the matched value — no binding is created.

```lisp
(def (_ mid _) (list 10 20 30))
mid   # => 20

(def [_ second _ fourth] [100 200 300 400])
second  # => 200
fourth  # => 400
```

## Nested patterns

Patterns compose to any depth, mixing list, array, and struct patterns.

```lisp
# list inside list
(def ((inner-a inner-b) outer-c) (list (list 1 2) 3))
inner-a   # => 1
outer-c   # => 3

# struct inside struct
(def {:config {:db {:host h}}}
  {:config {:db {:host "localhost"}}})
h         # => "localhost"

# struct containing array
(def {:point [_ target]} {:point [:skip :target]})
target    # => :target
```

## In function parameters

```lisp
(defn magnitude [{:x x :y y}]
  (+ x y))

(magnitude {:x 3 :y 4})   # => 7
```

## In let / let*

```lisp
(let [(a b) (list 10 20)]
  (+ a b))                 # => 30

# let* — sequential: second binding depends on first
(let* [(a b) (list 3 4)
       total (+ a b)]
  total)                    # => 7
```

## Mutable destructuring

`var` + destructuring creates mutable bindings.

```lisp
(var [mx my] [10 20])
(assign mx (+ mx my))
mx    # => 30
```

## Error semantics

Missing elements or keys signal an error. Use `match` for optional
patterns, or `protect` to capture the error.

```lisp
# (def (a b c) (list 1)) would error — not enough elements
# (def {:missing x} {:other 1}) would error — key not found

# extra elements are silently ignored
(def (p q) (list 1 2 3 4 5))
q     # => 2
```

---

## See also

- [destructuring-advanced.md](destructuring-advanced.md) — guards, match patterns
- [bindings.md](bindings.md) — def, var, let, let*, scope rules
- [match.md](match.md) — pattern matching with dispatch
- [functions.md](functions.md) — destructuring in function params
