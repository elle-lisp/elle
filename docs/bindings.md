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

## let — sequential bindings

Each binding sees all previous bindings (Clojure-style). Bindings are
flat pairs inside a single bracket form: `[name1 value1 name2 value2 ...]`.

Bindings are immutable unless prefixed with `@`.

```lisp
(let [x 5
      y (* x 2)          # y sees x
      z (+ x y)]         # z sees both x and y
  z)                       # => 15
```

```lisp
(let [@x 0]
  (assign x 10)
  x)                      # => 10
```

`let*` is kept as an alias for `let`.

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

`letrec` evaluates its initializers **left to right**, binding each before the
next runs, so a later initializer may use an earlier binding's *value*:

```lisp
(letrec [a 1
         b (+ a 1)]       # b sees a's value
  b)                       # => 2
```

This is `letrec*` semantics; `letrec*` is kept as an alias for `letrec`, exactly
as `let*` aliases `let`. Mutual recursion works because every name is in scope;
sequential value dependencies work because initialization is ordered.

**Use before initialization is an error**, not a silent `nil`. Referencing a
binding's *value* before its own initializer has run is a mistake:

```text
(letrec [a b           # b not yet initialized when a's RHS runs
         b 7]
  a)
# error: 'b' referenced before its initialization
```

A forward reference *through a function* is fine — the call happens after every
initializer has run:

```lisp
(letrec [a (fn [] b)     # defers the use of b until called
         b 7]
  (a))                    # => 7
```

**Defining the same name twice in one `letrec` is an error.** A duplicate has no
coherent meaning — earlier and forward references would bind the first
definition while later references bind the second, two contradictory meanings
for one name — so it is rejected:

```text
(letrec [x 1 x 2] x)
# error: duplicate binding 'x'
```

Duplicates are judged by **binding identity** — the name *and* its macro-hygiene
scope set — never by spelling. A binding introduced by a macro's template is a
different identity than one written at the call site, so the two coexist: each
side's references resolve to its own binding (see [macros.md](macros.md)).
`letrec` binders are hygiene-scoped exactly like every other binding form.

To re-use a name through a transformation, shadow it in a *sequential* context
(`let` / `do`), where each rebinding is an ordinary nested scope — refinement,
not redefinition:

```lisp
(let [x 1]
  (let [x (+ x 1)]
    (let [x (* x 10)] x)))   # => 20
```

## Function bodies are an implicit letrec

`def` and `var` forms in a function body are under the same strict `letrec*`
as an explicit `letrec`: every name is pre-bound (mutual recursion works),
initializers run left to right, and the same rules apply — referencing a
binding's *value* before its initializer has run is an error, and defining
the same name twice in one body is a duplicate-definition error (judged by
binding identity, as above — macro-introduced defines never collide with
user-written ones).

```text
((fn []
   (def x 1)
   (def x 2)   # error: duplicate binding 'x'
   x))
```

## Top-level implicit letrec

Top-level `def` and `defn` forms are under an implicit `letrec` (the same strict
`letrec*` as above). Order does not matter — functions can reference each other
freely.

```lisp
(defn ping [n]
  (if (= n 0) :done (pong (- n 1))))

(defn pong [n]
  (if (= n 0) :done (ping (- n 1))))

(ping 5)                  # => :done
```

Because the file is one `letrec*`, its rules apply: a forward *value* reference
before initialization is an error, and **defining the same top-level name twice
in one file is a duplicate-definition error** — almost always a mistake (or a
macro that forgot to `gensym` its helper). This is distinct from REPL
redefinition: a REPL evaluates each form as a *separate* top-level unit, so a
later `(def x …)` overwriting an earlier global is a property of the eval loop,
not an in-file duplicate, and remains allowed.

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
