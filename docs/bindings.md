# Bindings

Bindings associate names with values. Elle provides several binding forms,
each with different scope and mutability rules.

## Immutable by default

All bindings are **immutable by default**. Attempting to `assign` an
immutable binding is a compile-time error. To make a binding mutable,
prefix its name with `@`.

```lisp
(def x 10)           # immutable
(def @y 20)          # mutable
(assign y 30)        # ok
```

Assigning to an immutable binding is a compile error:

```text
(assign x 99)
# compile error: cannot assign immutable binding 'x' (use @x to make it mutable)
```

The `@` prefix appears only at the binding site — all subsequent uses of
the name omit it:

```lisp
(def @counter 0)
(assign counter 1)   # no @ here
counter              # => 1
```

## def — top-level binding

`def` creates a top-level binding. Without `@`, it is immutable.

```lisp
(def pi 3.14159)     # immutable
(def @counter 0)     # mutable
(assign counter (+ counter 1))
```

**`assign` is not `set`.** `set` creates a set collection. `assign` mutates
a binding.

## let — parallel bindings

All right-hand sides are evaluated in the outer scope. No binding sees
any other binding in the same `let`. Bindings are flat pairs inside a
single bracket form: `[name1 value1 name2 value2 ...]`.

Bindings are immutable unless prefixed with `@`.

```lisp
(def outer-a 100)
(let [a 1
      b (+ outer-a 1)]   # b sees outer-a (100), not a
  b)                       # => 101
```

```lisp
(let [@x 0]
  (assign x 10)
  x)                      # => 10
```

## let* — sequential bindings

Each binding sees all previous bindings. Use this when later bindings
depend on earlier ones. Bindings are immutable unless prefixed with `@`.

```lisp
(let* [x 5
       y (* x 2)         # y sees x
       z (+ x y)]        # z sees both x and y
  z)                       # => 15
```

## letrec — recursive bindings

Bindings can reference each other, enabling mutual recursion. Bindings
are immutable unless prefixed with `@`.

```lisp
(letrec [is-even (fn [n]
           (if (= n 0) true (is-odd (- n 1))))
         is-odd (fn [n]
           (if (= n 0) false (is-even (- n 1))))]
  (is-even 4))            # => true
```

## Top-level implicit letrec

Top-level `def` and `defn` forms are under an implicit `letrec`. Order
does not matter — functions can reference each other freely.

```lisp
(defn ping [n]
  (if (= n 0) :done (pong (- n 1))))

(defn pong [n]
  (if (= n 0) :done (ping (- n 1))))

(ping 5)                  # => :done
```

## Scope rules

### Lexical scope

A name is visible only in the block where it is defined. Inner scopes
can see outer names.

```lisp
(def outer-val 10)
(let [inner-val 20]
  (+ outer-val inner-val)) # => 30
```

### Shadowing

An inner binding hides an outer one. The outer value is untouched and
reappears when the inner scope ends.

```lisp
(def shade 1)
(let [shade 2]
  shade)                   # => 2
shade                      # => 1
```

### Closures capture their environment

```lisp
(defn make-adder [n]
  (fn [x] (+ x n)))

(def add5 (make-adder 5))
(add5 10)                  # => 15
```

### Mutable captures

When a mutable (`@`) binding is captured, mutations are visible to all
closures sharing that binding.

```lisp
(def @tally 0)
(def bump (fn [] (assign tally (+ tally 1))))
(bump)
(bump)
(bump)
tally                      # => 3
```

## Destructuring in bindings

All binding forms support destructuring. See
[destructuring.md](destructuring.md) for full coverage.

```lisp
(def [da db dc] [10 20 30])
da                         # => 10
dc                         # => 30
```

---

## See also

- [destructuring.md](destructuring.md) — unpacking collections in bindings
- [functions.md](functions.md) — fn, defn, closures, composition
- [control.md](control.md) — conditionals, loops, early exit
