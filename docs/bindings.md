# Bindings

<!-- audited: 2026-11-18 -->

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
(assert (= y 30))
```

Assigning to an immutable binding is a compile error:

```lisp
(defn compile-error [src]
  (let [[ok? err] (protect (compile/whole-module src "<doc>"))]
    (assert (not ok?))
    (get err :message)))

(assert (string/contains? (compile-error "(def x 10) (assign x 99)")
                          "cannot assign immutable binding 'x' (use @x to make it mutable)"))
```

The `@` prefix appears only at the binding site — all subsequent uses of
the name omit it:

```lisp
(def @counter 0)
(assign counter 1)   # no @ here
(assert (= counter 1))
```

## def — a binding in a body

`def` binds a name in the body it stands in: a function body, a `let` body or
the file. Without `@`, it is immutable. `var` binds a mutable name.

```lisp
(def pi 3.14159)     # immutable
(def @hits 0)        # mutable
(assign hits (+ hits 1))
(assert (= hits 1))
```

**`assign` is not `set`.** `set` creates a set collection. `assign` mutates
a binding.

## let — sequential bindings

Each binding sees all previous bindings (Clojure-style). Bindings are
flat pairs inside a single bracket form: `[name1 value1 name2 value2 ...]`.

Bindings are immutable unless prefixed with `@`.

```lisp
(assert (= (let [x 5
                 y (* x 2)          # y sees x
                 z (+ x y)]         # z sees both x and y
             z)
           15))

(assert (= (let [@x 0]
             (assign x 10)
             x)
           10))
```

`let*` is kept as an alias for `let`.

## letrec — recursive bindings

Bindings can reference each other, enabling mutual recursion. Bindings
are immutable unless prefixed with `@`.

```lisp
(assert (letrec [is-even (fn [n]
                           (if (= n 0) true (is-odd (- n 1))))
                 is-odd (fn [n]
                          (if (= n 0) false (is-even (- n 1))))]
          (is-even 4)))
```

`letrec` evaluates its initializers **left to right**, binding each before the
next runs, so a later initializer may use an earlier binding's *value*:

```lisp
(assert (= (letrec [a 1
                    b (+ a 1)]       # b sees a's value
             b)
           2))
```

This is `letrec*` semantics under the name `letrec`; there is no separate
`letrec*` form. Mutual recursion works because every name is in scope;
sequential value dependencies work because initialization is ordered.

**Use before initialization is an error**, not a silent `nil`. Referencing a
binding's *value* before its own initializer has run is a mistake:

```lisp
(assert (string/contains? (compile-error "(letrec [a b  b 7] a)")
                          "'b' referenced before its initialization"))
```

A forward reference *through a function* is fine — the call happens after every
initializer has run:

```lisp
(assert (= (letrec [a (fn [] b)     # defers the use of b until called
                    b 7]
             (a))
           7))
```

**Defining the same name twice in one `letrec` is an error.** A duplicate has no
coherent meaning — earlier and forward references would bind the first
definition while later references bind the second, two contradictory meanings
for one name — so it is rejected:

```lisp
(assert (string/contains? (compile-error "(letrec [x 1 x 2] x)")
                          "duplicate binding 'x'"))
```

Duplicates are judged by **binding identity** — the name *and* its macro-hygiene
scope set — never by spelling. A binding introduced by a macro's template is a
different identity than one written at the call site, so the two coexist: each
side's references resolve to its own binding (see [macros.md](macros.md)).
`letrec` binders are hygiene-scoped exactly like every other binding form.

To re-use a name through a transformation, shadow it with a nested `let`, where
each rebinding is an ordinary nested scope — refinement, not redefinition:

```lisp
(assert (= (let [x 1]
             (let [x (+ x 1)]
               (let [x (* x 10)] x)))
           20))
```

## A name exists from its def onward

A `def` binds its name from its own form to the end of the body that holds it.
A read before the `def` finds the binding outside the body, and with none it
is a compile error. The file is a body too, so a top-level `def` follows the
same rule. A function cannot call a function defined after it.

```lisp
(def level 1)
(defn deeper []
  (def before level)
  (def level 5)
  [before level])
(assert (= (deeper) [1 5]) "the read before the def finds the file's level")

(assert (string/contains? (compile-error "(defn f [] (g)) (defn g [] 1)")
                          "undefined variable: g"))
```

A lambda in a `def`'s value reads the binding that `def` makes, so a function
can call itself. A direct read in the value runs before the binding holds a
value, and is a compile error:

```lisp
(defn countdown [n] (if (= n 0) :done (countdown (- n 1))))
(assert (= (countdown 3) :done))

(assert (string/contains? (compile-error "(defn f [] (def x 1) (def x (+ x 1)) x)")
                          "'x' referenced before its initialization"))
```

Functions that call each other bind in one `letrec`:

```lisp
(defn parity [n]
  (letrec [ev? (fn [k] (if (= k 0) true (od? (- k 1))))
           od? (fn [k] (if (= k 0) false (ev? (- k 1))))]
    (ev? n)))
(assert (= (parity 10) true))
(assert (= (parity 7) false))
```

In a file that declares epoch 17 or earlier, a body binds every name its `def`
forms bind before any form runs, and a direct read in a `def`'s value reads the
binding before it. The compiler moves a function's `def` above the first form
that reads it inside a lambda, where the move cannot change what a form does.
Functions that call each other, and a read of a `def` that cannot move, bind in
one `letrec` from the reading `def` through the `def` it reads. A `def` whose
value reads its own name becomes a `let` over the forms after it. A forward read
it cannot carry across is a compile error ([epochs](epochs.md)).

```lisp
(defn run [src]
  ((get (get (compile/whole-module src "<doc>") 0) 1)))

(def forward "(defn f [] (g))\n(defn g [] 1)\n(f)")
(assert (= (run (string "(elle/epoch 17)\n" forward)) 1)
        "epoch 17 moves g above f")
(assert (string/contains? (compile-error forward) "undefined variable: g"))
(def mutual (string "(elle/epoch 17)\n"
                    "(defn ev? [k] (if (= k 0) true (od? (- k 1))))\n"
                    "(defn od? [k] (if (= k 0) false (ev? (- k 1))))\n"
                    "(ev? 4)"))
(assert (= (run mutual) true)
        "epoch 17 reads functions that call each other as a letrec")
(assert (= (run "(elle/epoch 17)\n(def x 1)\n(def x (+ x 1))\nx") 2)
        "epoch 17 reads the redefinition as a let")
(assert (string/contains?
          (compile-error "(elle/epoch 17)\n(defn f [] n)\n(println 1)\n(def n 1)\n(f)")
          "'n' is read before its def")
        "a def whose value is no function stays, and the letrec cannot hold the expression")
```

A body may define a name again. Each `def` makes a new binding, code after it
reads that binding, and a function keeps the binding it saw where it was
written.

```lisp
(defn redefine []
  (def x 3)
  (def p (fn [] (= x 3)))
  (def x 4)
  [x (p)])
(assert (= (redefine) [4 true]))
```

Every `def` of a name in one scope agrees on mutability. A `var` and a `def`
with `@` are mutable, and a mix is a compile error. A parameter is not a `def`,
so `(def @x x)` may give a parameter a mutable copy.

```lisp
(assert (string/contains? (compile-error "((fn [] (def x 1) (def @x 2) x))")
                          "both mutable and immutable"))
```

A REPL evaluates each form as a separate unit, so a later `(def x …)` there
replaces an earlier global. That is a property of the eval loop.

## Where a def binds

A `def` or `var` binds only where it stands directly in a body. A body is the
body of a function, `let`, `letrec`, `block`, `parameterize` or a `match` arm,
or the file. `begin` opens no scope ([control](control.md)), so a `begin` that
stands directly in a body passes its forms to that body.

```lisp
(defn through-begin []
  (begin (defn helper [] :found))
  (helper))
(assert (= (through-begin) :found))

(def *depth* (make-parameter 0))
(defn scoped []
  (parameterize ((*depth* 1))
    (def seen (*depth*))
    seen))
(assert (= (scoped) 1) "a parameterize body is a scope of its own")
```

A `def` anywhere else is a compile error. That is a branch of `if`, `cond` or
`when`, a `while` body, an `and` or `or` operand, an argument, a binding's
value, or a `begin` that stands in one of those. A `def` there could be skipped or
repeated, and a reader could not tell which binding a later read finds. The
message names the form that holds the `def` and where that form stands. Write
a `let` or a `block` there instead. In a file that declares epoch 16 or earlier,
the compiler moves such a `def` into a `let` where the move keeps its meaning
([epochs](epochs.md)).

```lisp
(assert (string/contains? (compile-error "(defn f [c] (when c (def x 1)) 2)")
                          "a def binds only directly in a body"))
(assert (string/contains? (compile-error "(defn f [] (while true (var x 1)))")
                          "a var binds only directly in a body"))
(assert (string/contains? (compile-error "(def k (do (defn inner [] 1) (inner)))")
                          "a def binds only directly in a body"))

(defn branch-local [c]
  (when c (let [x 1] (+ x 1))))
(assert (= (branch-local true) 2))
```

## Scope rules

### Lexical scope

A name is visible only in the block where it is defined. Inner scopes
can see outer names.

```lisp
(def outer-val 10)
(assert (= (let [inner-val 20]
             (+ outer-val inner-val))
           30))
```

### Shadowing

An inner binding hides an outer one. The outer value is untouched and
reappears when the inner scope ends.

```lisp
(def shade 1)
(assert (= (let [shade 2]
             shade)
           2))
(assert (= shade 1))
```

### Closures capture their environment

```lisp
(defn make-adder [n]
  (fn [x] (+ x n)))

(def add5 (make-adder 5))
(assert (= (add5 10) 15))
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
(assert (= tally 3))
```

## Destructuring in bindings

All binding forms support destructuring. See
[destructuring.md](destructuring.md) for full coverage.

```lisp
(def [da db dc] [10 20 30])
(assert (= da 10))
(assert (= dc 30))
```

---

## See also

- [destructuring.md](destructuring.md) — unpacking collections in bindings
- [functions.md](functions.md) — fn, defn, closures, composition
- [control.md](control.md) — conditionals, loops, early exit
