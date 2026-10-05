# Signal Inference

<!-- audited: 2026-10-04 -->

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
5. A construct that checks something at run time, and raises `:error` when
   the check fails. The list is below.

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

### What raises

Every signal is a possible raise, so a construct that can raise carries
`:error`. A function whose inferred signal is empty therefore cannot raise,
and that is what makes `silent` a promise rather than a guess. These
constructs carry `:error`:

- Strict destructuring: the patterns of `def`, `var`, `let` and `letrec`, a
  required parameter pattern, and a `&keys` pattern. An `&opt` or `&named`
  pattern binds `nil` instead of raising, so it adds nothing.
- Qualified access, `m:k`, which is a `get`.
- A spliced call, `(f ;xs)`: the argument count is checked at run time, and
  the spliced value must be a sequence.
- A call to a function with a `&keys` or `&named` collector: the keyword
  arguments are checked at the call.
- `parameterize`, whose bindings must name parameters.
- `eval`, whose signal is exactly `:error`. It runs the datum on the calling
  fiber and holds no park of that code, so a yield, an I/O request or a halt
  inside it comes back as an `:eval-error`.
- A `(silence p)` bound, whose entry check may raise.
- A `b[..]` literal unless every element is an integer literal from 0 to 255,
  and a `{..}` or `@{..}` literal unless every key is a literal: a mutable key
  is rejected at run time.

```lisp
(defn bits-of [src name] (get (signal-of src name) :bits))

(assert (= |:error| (bits-of "(defn f [x] (def [a b] x) a)" :f)))
(assert (= |:error| (bits-of "(defn f [m] m:k)" :f)))
(assert (= |:error| (bits-of "(defn g [a] a) (defn f [xs] (g ;xs))" :f)))
(assert (= |:error| (bits-of "(defn g [&named a] a) (defn f [] (g :a 1))" :f)))
(assert (= |:error| (bits-of "(defn f [p] (parameterize ((p 1)) 2))" :f)))
(assert (= |:error| (bits-of "(defn f [] (eval '(+ 1 2)))" :f)))
(assert (= |:error| (bits-of "(defn f [x] b[x])" :f)))
(assert (= || (bits-of "(defn f [v] {:a v})" :f)) "a literal key cannot fail")
```

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
| `(muffle spec)` | Squelch the function over `spec` when a closure is made from it |
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

`(silence p)` bounds one parameter. The function no longer takes that
parameter's signal from the argument, so a higher-order function stops being
polymorphic. The bound is checked at function entry, and the check may raise,
so the function carries `:error` and nothing else. Several `(silence p)`
forms may appear, one per parameter. A name that is not a parameter is a
compile error.

```lisp
(def sig (signal-of "(defn map-silent [f xs] (silence f) (map f xs))"
                    :map-silent))
(assert (= |:error| (get sig :bits)) "the entry check may raise")
(assert (empty? (get sig :propagates)) "map-silent takes no signal from f")

(assert (string/contains? (compile-error '(fn [x] (silence z) x))
                          "'z' is not a parameter of this function"))
```

The compiler does not check the argument at the call site. At entry, a
closure is checked by its inferred signal and a native function by its
declared signal; any other value passes, and calling it raises inside the
function, which the `:error` covers. A value whose signal is not empty fails
the check with `:signal-violation`:

```lisp
(defn apply-silent [f x]
  (silence f)
  (f x))

(assert (= 42 (apply-silent (fn [x] x) 42)))
(assert (= false (apply-silent callable? 42)) "a silent native passes")

(def [ok? err] (protect (apply-silent (fn [x] (yield x)) 42)))
(assert (= :signal-violation (get err :error)))
(assert (string/contains? (get err :message)
                          "closure may emit {:yield} but parameter is restricted to {}"))

(def [ok? err] (protect (apply-silent + 42)))
(assert (not ok?) "+ may raise :error, so it is not silent")

(def [ok? err] (protect (apply-silent length [1 2])))
(assert (string/contains? (get err :message)
                          "length may emit {:error} but parameter is restricted to {}"))
```

A violation nothing catches reports as any uncaught error does, at the call
that passed the value.

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

`(muffle spec)` is `squelch` applied to the function itself, when a closure is
made from it. The inferred signal loses the bits of `spec` that the body may
raise and gains `:error` in their place, exactly as a squelch narrows a
closure. At run time the function's boundary turns a muffled signal into a
`:signal-violation` error, on every tier a squelch boundary covers.

```lisp
(def sig (signal-of "(defn f [] (muffle :yield) (yield 1) 2)" :f))
(assert (= |:error| (get sig :bits)) "the muffled yield comes back as an error")

(def sig (signal-of "(defn f [x] (muffle :yield) x)" :f))
(assert (get sig :silent) "a muffle of a signal the body never raises changes nothing")

(defn muffled [] (muffle :yield) (yield 1) 2)
(def [ok? err] (protect (muffled)))
(assert (= :signal-violation (get err :error)))
(assert (= "squelch: signal {:yield} caught at boundary" (get err :message)))
```

`:error` and `:halt` pass every boundary, so neither can be muffled; the form
is a compile error. A ceiling is checked against the signal after the muffle,
so `(silence)` rejects a muffled body: the violation it raises is a raise.
`(attune! :error)` admits it.

```lisp
(assert (string/contains? (compile-error '(fn [x] (muffle :error) (+ x 1)))
                          "{:error} passes every boundary and cannot be muffled"))

(assert (string/contains?
          (compile-error '(fn [] (silence) (muffle :yield) (yield 1)))
          "function restricted to {} but body may emit {:error}"))

(defn bounded [] (attune! :error) (muffle :yield) (yield 1) 2)
(assert (= :signal-violation (get (get (protect (bounded)) 1) :error)))
```

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

## The boundary of a silent function

A function whose inferred signal is empty is enforced at its boundary as a
closure squelched over every signal. Inference is sound, so the boundary
fires only on a defect in the compiler, and then it raises
`:signal-violation` where the abort it replaces killed the process. `:error`
and `:halt` pass this boundary as they pass every other, so a raise that
nothing catches reports as an ordinary uncaught error whatever the function's
inferred signal.

The mask a closure's boundary enforces is therefore three things: the mask
`squelch` or `attune` gave it, the function's `muffle`, and every signal when
the function is silent. One method on the closure answers it, and every
enforcement site in the interpreter and the JIT asks that method.

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
