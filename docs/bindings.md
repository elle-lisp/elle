# Bindings

<!-- audited: 2026-11-09 -->

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

## def — top-level binding

`def` creates a top-level binding. Without `@`, it is immutable.

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

## Function bodies are an implicit letrec

A function body pre-binds every name its `def` and `var` forms bind, so a
function written earlier in the body can call a later `def`. Initializers run
left to right. A body may define a name again: each `def` makes a new binding,
code after it reads that binding, and a function keeps the binding it saw where
it was written.

```lisp
(defn redefine []
  (def x 3)
  (def p (fn [] (= x 3)))
  (def x 4)
  [x (p)])
(assert (= (redefine) [4 true]))
```

Until the body's first `def` of a name runs, the name reads the binding the body
sees from outside. A direct read before that `def` reads it. So does a function
written before the `def` and called before it runs, which reads `nil` where no
outer binding exists. A direct read with no outer binding is an error:

```lisp
(def level 1)
(defn deeper []
  (def level (+ level 1))
  level)
(assert (= (deeper) 2))

(def greeting :outer)
(defn early-call []
  (defn say [] greeting)
  (def before (say))
  (def greeting :inner)
  [before (say)])
(assert (= (early-call) [:outer :inner]))

(assert (string/contains? (compile-error "((fn [] (def a b) (def b 7) a))")
                          "'b' referenced before its initialization"))
```

Every `def` of a name in one scope agrees on mutability. A `var` and a `def`
with `@` are mutable, and a mix is a compile error. A parameter is not a `def`,
so `(def @x x)` may give a parameter a mutable copy.

```lisp
(assert (string/contains? (compile-error "((fn [] (def x 1) (def @x 2) x))")
                          "both mutable and immutable"))
```

## Top-level implicit letrec

Top-level `def` and `defn` forms are under an implicit `letrec`. Order does
not matter — functions can reference each other freely.

```lisp
(defn ping [n]
  (if (= n 0) :done (pong (- n 1))))

(defn pong [n]
  (if (= n 0) :done (ping (- n 1))))

(assert (= (ping 5) :done))
```

The file redefines a name as a function body does, and its `def` forms of one
name agree on mutability too. A form or a function written after a redefinition
reads it, and one written before it reads the earlier definition.

A read before a name's first `def` differs. The file reads the file's own
binding there, so it gets `nil` instead of the outer binding or a compile error
([#1244](https://github.com/elle-lisp/elle/issues/1244)). A REPL is a different
case: it evaluates each form as a *separate* top-level unit, so a later
`(def x …)` overwriting an earlier global is a property of the eval loop.

```lisp
(defn compiles? [src]
  (first (protect (compile/whole-module src "<doc>"))))

(assert (compiles? "(def a b) (def b 1)"))
(assert (compiles? "(def twice 1) (def twice 2)"))
(assert (not (compiles? "(def @v @[1]) (def v (freeze v))")))

(def again 1)
(defn read-first [] again)
(def again 2)
(defn read-second [] again)
(assert (= (read-first) 1))
(assert (= (read-second) 2))
```

## Where a def binds

A `def` or `var` binds in the innermost form around it that opens a scope: a
function body, `let`, `letrec`, `block`, a `match` arm, or the file. `begin`
opens none ([control](control.md)), and neither does `if`, `when`, `cond`,
`and`, `or`, `while` or a call. A `def` nested in such forms obeys the rules of
the scope it binds in, as if it stood there directly.

```lisp
(defn nested-forward []
  (defn call-helper [] (helper))
  (begin (defn helper [] :found))
  (call-helper))
(assert (= (nested-forward) :found))

(when false (def skipped 1))
(def skipped 7)
(defn read-skipped [] skipped)
(assert (= skipped 7))
(assert (= (read-skipped) 7))
```

A `def` in a branch, a loop body or a short-circuit operand may not run. After
such a form, a read of the name finds the latest `def` of it that ran. Where
none ran, the read finds the binding visible before the form, or `nil` where
there is none. Inside a loop, a read finds the `def` of this pass or an earlier
one.

```lisp
(defn branch [c]
  (def x 1)
  (def p (fn [] x))
  (when c (def x 2))
  [x (p)])
(assert (= (branch true) [2 1]))
(assert (= (branch false) [1 1]))

(def shade-level 1)
(defn maybe-shade [c]
  (when c (def shade-level 2))
  shade-level)
(assert (= (maybe-shade true) 2))
(assert (= (maybe-shade false) 1))

(defn maybe-found [c]
  (when c (def found 1))
  found)
(assert (nil? (maybe-found false)))

(defn last-pass []
  (def @i 0)
  (def seen :none)
  (while (< i 3)
    (def seen i)
    (assign i (+ i 1)))
  seen)
(assert (= (last-pass) 2))
```

Such a read stands for whichever binding is current when it runs. Where any
binding it may find is mutable, the read is a compile error, and so is an
`assign`. Where all of them are immutable, an `assign` fails as for any
immutable binding.

```lisp
(assert (string/contains? (compile-error "(fn [c] (when c (def @n 0)) n)")
                          "one of them is mutable"))
(assert (string/contains? (compile-error "(when true (def kept 1)) (assign kept 2)")
                          "cannot assign immutable binding 'kept'"))
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
