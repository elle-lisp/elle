# Signal Inference

<!-- audited: 2026-09-22 -->

How the compiler infers each function's signal, and the forms that bound,
narrow or check it.

The examples below read a signal with `compile/signal`, and they catch a
compile error with this helper:

```lisp
(defn compile-error [form]
  "The message eval reports when form does not compile."
  (let [[ok? err] (protect (eval form))]
    (assert (not ok?) "the form compiles")
    (get err :message)))

(defn signal-of [src name]
  (compile/signal (compile/analyze src) name))
```

## What the compiler infers

Every function carries an inferred signal with two parts. `:bits` is the set
of signals the function may emit. `:propagates` is the set of parameter
positions whose argument's signal the function passes on when it calls them.

The analyzer accumulates both from the body:

1. A direct `emit`, and the macros over it such as `yield` and `error`.
2. A call to a function whose signal is known: a primitive, a binding in this
   file, or a squelched closure (see below).
3. A call to a parameter: that parameter's position joins `:propagates`,
   unless `(silence p)` bounds the parameter.
4. A call to anything else, such as a mutable binding or a computed callee:
   every bit a user program can raise.

```lisp
(def sig (signal-of "(defn gen [] (yield 1))" :gen))
(assert (= |:yield| (get sig :bits)))

(def sig (signal-of "(defn call [f x] (f x))" :call))
(assert (= |0| (get sig :propagates)) "call passes on parameter 0's signal")

(def sig (signal-of "(def @h nil) (defn via [x] (h x))" :via))
(assert (contains? (get sig :bits) :io) "an unknown callee may do anything")
```

Mutually recursive definitions in one file converge by a fixpoint; see
[pipeline.md](../pipeline.md). The signal decides the calling convention: a
call whose callee may yield, do I/O or wait keeps a continuation frame.

## Declarations inside a function

Seven forms declare something about the function they appear in. Each is
legal anywhere inside a function body and applies to the innermost enclosing
function. Each evaluates to `nil`. Outside every function, each is a compile
error.

| Form | Declares |
|------|----------|
| `(silence)` | The function emits nothing, `:error` included |
| `(silence p)` | Parameter `p` must be silent |
| `(attune! spec)` | The function emits at most `spec` |
| `(muffle spec)` | Remove `spec` from the function's inferred signal |
| `(silent!)` | Assert that the inferred signal is empty |
| `(numeric!)` | Assert that the function is GPU-eligible |
| `(immutable! x)` | Assert that binding `x` is never assigned |

`spec` is a signal keyword or a literal set of them.

```lisp
(assert (string/contains? (compile-error '(silence))
                          "silence must appear inside a function body"))
```

### `(silence)`

`(silence)` sets the function's ceiling to the empty set. A body that may emit
anything, `:error` included, is a compile error. Generic arithmetic may raise
`:error` on a non-number, so `(silence)` rejects it.

```lisp
(defn select [flag a b]
  (silence)
  (if flag a b))
(assert (= 1 (select true 1 2)))

(assert (string/contains?
          (compile-error '(fn [x y] (silence) (+ x y)))
          "function restricted to {} but body may emit {:error}"))

(assert (string/contains? (compile-error '(fn [x] (silence :yield) x))
                          "silence takes no signal keywords"))
```

### `(silence p)`

`(silence p)` bounds one parameter. The function no longer propagates that
parameter's signal, so a higher-order function becomes silent. Several
`(silence p)` forms may appear, one per parameter. A name that is not a
parameter is a compile error.

```lisp
(def sig (signal-of "(defn map-silent [f xs] (silence f) (map f xs))"
                    :map-silent))
(assert (get sig :silent))

(assert (string/contains? (compile-error '(fn [x] (silence z) x))
                          "'z' is not a parameter of this function"))
```

The bound is checked at run time, at function entry. The compiler does not
check the argument at the call site. A closure whose signal is not empty
fails the check with `:signal-violation`:

```lisp
(defn apply-silent [f x]
  (silence f)
  (f x))

(assert (= 42 (apply-silent (fn [x] x) 42)))

(def [ok? err] (protect (apply-silent (fn [x] (yield x)) 42)))
(assert (= :signal-violation (get err :error)))
(assert (string/contains? (get err :message)
                          "closure may emit {:yield} but parameter is restricted to {}"))

(def [ok? err] (protect (apply-silent + 42)))
(assert (not ok?) "+ may raise :error, so it is not silent")
```

A violation that nothing catches aborts the program with a panic that names
`(silence)` instead of the call (#1233).

### `(attune! spec)`

`(attune! spec)` sets the ceiling to `spec` instead of the empty set.
`(silence)` is the ceiling with nothing in it. A function that may fail but
must not suspend declares `(attune! :error)`.

```lisp
(defn add [x y]
  (attune! :error)
  (+ x y))
(assert (= 3 (add 1 2)))

(defn parse [input]
  (attune! |:yield :error|)
  (if (empty? input)
    (error {:error :parse-error})
    (yield (first input))))

(assert (string/contains?
          (compile-error '(fn [] (attune! :yield) (println "oops")))
          "function restricted to {:yield} but body may emit {:error"))
```

### `(muffle spec)`

`(muffle spec)` removes `spec` from the inferred signal. Beside a ceiling, it
widens the ceiling instead: `(silence) (muffle :error)` accepts a body that
may raise `:error`, and the function still infers silent.

```lisp
(def sig (signal-of "(defn f [x] (silence) (muffle :error) (+ x 1))" :f))
(assert (get sig :silent))
```

Nothing enforces a muffle at run time. A muffled signal still leaves the
function, and a caller compiled against the narrower signal is not ready for
it (#1236).

### Assertions

`(silent!)` asserts that the inferred signal is empty. The check runs before a
ceiling or a muffle applies, so it states what the body does, not what the
function declares.

```lisp
(defn tight [x] (silent!) (if x 1 2))
(assert (= 1 (tight true)))

(assert (string/contains? (compile-error '(fn [x] (silent!) (+ x 1)))
                          "silent! assertion failed: function may emit {:error}"))
```

`(numeric!)` asserts that the function is GPU-eligible. The lowered body may
hold only numeric instructions and control flow: no call, no closure, no
heap value and no `emit`. Generic `+` is a call, so the body uses the `%`
intrinsics. The assertion also marks every parameter as a number, which is
the proof an intrinsic demands of its operands; see
[intrinsics.md](../intrinsics.md). Nothing checks the argument at run time,
so a non-number gives an unchecked result.

```lisp
(defn square [x] (numeric!) (%mul x x))
(assert (= 2.25 (square 1.5)))
(assert (get (protect (square "a")) 0) "a non-number argument raises nothing")

(assert (string/contains? (compile-error '(fn [x] (numeric!) (+ x 1)))
                          "numeric! assertion failed: function is not GPU-eligible"))
```

`(immutable! x)` asserts that the body never assigns `x`. A binding without
`@` cannot be assigned anyway, so the assertion matters for a mutable one.

```lisp
(assert (string/contains?
          (compile-error '(fn [@x] (immutable! x) (assign x 2) x))
          "immutable! assertion failed: 'x' is assigned in body"))
```

## Runtime transforms: `squelch` and `attune`

`squelch` and `attune` are primitives, not declarations. Each returns a new
closure that shares the original's bytecode and environment. When the new
closure returns a signal its mask covers, the boundary turns that signal into
a `:signal-violation` error.

- `(squelch f mask)` blocks the signals in `mask` and lets the rest through.
  `mask` is a keyword, a set, an array or list of keywords, or an integer.
- `(attune mask f)` takes the mask first. It lets through only the signals in
  `mask` and blocks the rest.

```lisp
(defn producer [] (yield 1))

(def [ok? err] (protect ((squelch producer :yield))))
(assert (= :signal-violation (get err :error)))
(assert (= "squelch: signal {:yield} caught at boundary" (get err :message)))

(def [ok? err] (protect ((attune |:yield :error| (fn [] (println "hi"))))))
(assert (= "squelch: signal {:io} caught at boundary" (get err :message)))
```

Layers compose: a squelch of a squelched closure blocks both masks. The check
also fires when the squelched closure is called in tail position.

```lisp
(def quiet (squelch (squelch (fn [] (println "hi")) :yield) :io))
(assert (= :signal-violation (get (get (protect (quiet)) 1) :error)))

(def blocked (squelch producer :yield))
(defn tail-call [] (blocked))
(assert (= :signal-violation (get (get (protect (tail-call)) 1) :error)))
```

Three classes of signal cross every boundary untouched. `:error` and `:halt`
pass whole, so squelching `:error` does nothing. The VM's `:switch`
trampoline passes, and the pause bits such as `:fuel` are subtracted from the
mask. [signals/mod.rs](../../src/signals/mod.rs) holds the one predicate,
`squelched_bits`, that every tier asks.

```lisp
(def [ok? err] (protect ((squelch (fn [] (error {:error :mine})) :error))))
(assert (= :mine (get err :error)) "squelch never blocks :error")
```

### Narrowing at compile time

When a `def` or `let` binds `(squelch f mask)` or `(attune mask f)` with a
literal mask, the analyzer computes the new closure's signal. It removes the
blocked bits, and adds `:error` when it removed any. A caller of the binding
then sees the narrower signal.

```lisp
(def sig (signal-of "(defn p [] (yield 1))
(def safe (squelch p :yield))
(defn use [] (safe))" :use))
(assert (= |:error| (get sig :bits)))
```

A squelch that removes a bit adds `:error`, and `:error` cannot be squelched.
So a squelch never turns a closure that signals into a silent one.

## Across files: signal projection

When a file returns a struct literal of closures, or a function whose body is
one, the compiler records a projection: a signal for each field. The
analyzer unwraps `begin`, `let`, `letrec` and `fn` bodies to find the struct,
and takes the union of the two branches of an `if`. Any other return shape
gives no projection.

An importing file that binds `((import "literal"))` looks the projection up.
Today the projected signal never reaches a call through `module:field`, so a
call into another file is treated as unknown (#1232). A `(silence)` function
cannot call into another module until that is fixed.

## See also

- [Signal index](index.md)
- [Signals and JIT](jit.md)
- [Design philosophy](../philosophy.md) — why higher-order functions are
  polymorphic by default
