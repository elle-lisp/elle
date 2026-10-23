# Signal Inference

<!-- audited: 2026-10-22 -->

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
   unless `(bound p spec)` bounds the parameter, which contributes `spec`
   instead. The call itself may raise, so it carries `:error` (see below).
4. A call to anything else, such as a mutable binding or a computed callee:
   every bit a user program can raise.
5. A construct that checks something at run time, and raises `:error` when
   the check fails. The list is below.

A binding in this file that names a primitive, as `(def eqv %eq)` does, has
that primitive's signal and arity. A call through it signals what a call to the
primitive does.

```lisp
(def sig (signal-of "(defn gen [] (yield 1))" :gen))
(assert (= |:yield| (get sig :bits)))

(def sig (signal-of "(def eqv %eq) (defn same? [a b] (eqv a b))" :same?))
(assert (get sig :silent) "a call through eqv signals what %eq does")

(def sig (signal-of "(defn call [f x] (f x))" :call))
(assert (= |0| (get sig :propagates)) "call passes on parameter 0's signal")
(assert (= |:error| (get sig :bits)) "the call to f may raise")

(def sig (signal-of "(defn call [f x] (yield x) (f x))
(defn twice [k] (call k 1))" :twice))
(assert (= |0| (get sig :propagates)) "twice passes on k's signal through call")
(assert (= |:yield :error| (get sig :bits)) "and keeps what call raises itself")

(def sig (signal-of "(def @h nil) (defn via [x] (h x))" :via))
(assert (contains? (get sig :bits) :io) "an unknown callee may do anything")
```

Mutually recursive definitions in one file or one `letrec` converge by a
fixpoint; see [pipeline.md](../pipeline.md). So does a function that calls
itself from a lambda inside its own body, wherever it is defined. The signal
decides the calling convention: a call whose callee may yield, do I/O or wait
keeps a continuation frame.

```lisp
(assert (string/contains?
          (compile-error '(defn outer []
                            (defn walk [n]
                              (if (= n 0)
                                (yield n)
                                ((fn [k] (silent!) (walk k)) (- n 1))))
                            walk))
          "function restricted to {} but body may emit {:error, :yield}")
        "the lambda inside walk takes the signal of walk")
```

### What raises

Every signal is a possible raise, so a construct that can raise carries
`:error`. A function whose inferred signal is empty therefore cannot raise,
and that is what makes a silent function a proof rather than a guess. These
constructs carry `:error`:

- Strict destructuring: the patterns of `def`, `var`, `let` and `letrec`, a
  required parameter pattern, and a `&keys` pattern. An `&opt` or `&named`
  pattern binds `nil` instead of raising, so it adds nothing.
- Qualified access, `m:k`, which is a `get`.
- A call whose argument count the compiler did not check: a spliced call
  `(f ;xs)`, and a call to a callee whose arity the compiler does not know,
  such as a parameter or a binding of `squelch`. The spliced value must also
  be a sequence. A primitive, a stdlib function and a function defined in
  the file have a known arity, so a call to one with the right count adds
  nothing, and a call with the wrong count is a compile error.
- A collection literal with a splice, `[1 ;xs]`: the spliced value must be
  a sequence.
- A call to a function with a `&keys` or `&named` collector: the keyword
  arguments are checked at the call.
- `parameterize`, whose bindings must name parameters.
- `eval`, whose signal is exactly `:error`. It runs the datum on the calling
  fiber and holds no park of that code, so a yield, an I/O request or a halt
  inside it comes back as an `:eval-error`.
- A `(bound p spec)` bound, whose entry check may raise.
- A `b[..]` literal unless every element is an integer literal from 0 to 255,
  and a `{..}` or `@{..}` literal unless every key is a literal: a mutable key
  is rejected at run time.

```lisp
(defn bits-of [src name] (get (signal-of src name) :bits))

(assert (= |:error| (bits-of "(defn f [x] (def [a b] x) a)" :f)))
(assert (= |:error| (bits-of "(defn f [m] m:k)" :f)))
(assert (= |:error| (bits-of "(defn g [a] a) (defn f [xs] (g ;xs))" :f)))
(assert (= |:error| (bits-of "(defn p [] 1) (def q (squelch :yield p))
(defn f [] (q))" :f)) "q's arity is not known")
(assert (= || (bits-of "(defn f [x] (identity x))" :f)) "identity's arity is")
(assert (= |:error| (bits-of "(defn f [x] [1 ;x])" :f)))
(assert (= |:error| (bits-of "(defn g [&named a] a) (defn f [] (g :a 1))" :f)))
(assert (= |:error| (bits-of "(defn f [p] (parameterize ((p 1)) 2))" :f)))
(assert (= |:error| (bits-of "(defn f [] (eval '(+ 1 2)))" :f)))
(assert (= |:error| (bits-of "(defn f [x] b[x])" :f)))
(assert (= || (bits-of "(defn f [v] {:a v})" :f)) "a literal key cannot fail")

(assert (string/contains? (compile-error '(fn [] (identity 1 2)))
                          "arity error: identity expects 1 argument, got 2"))
```

## Declarations inside a function

Five forms declare something about the function they appear in. Each is legal
anywhere inside a function body and applies to the innermost enclosing
function. Each evaluates to `nil`. Outside every function, each is a compile
error.

| Form | Declares |
|------|----------|
| `(attune! spec)` | The function emits at most `spec` |
| `(silent!)` | The function emits nothing: `(attune! \|\|)` |
| `(bound p spec)` | The argument for parameter `p` emits at most `spec` |
| `(numeric!)` | The function is GPU-eligible |
| `(immutable! x)` | Binding `x` is never assigned |

`spec` is a signal keyword or a literal set of them. The empty set, `||`, is a
spec. Inside quoted data, a set literal becomes the list `(set :yield :io)`. A
spec reads that list as the set it spells, so a form built as data and handed
to `eval` compiles as written.

```lisp
(assert (string/contains? (compile-error '(silent!))
                          "silent! must appear inside a function body"))

(def [ok? _] (protect (eval '(fn [] (attune! |:yield :io|) (yield 1)))))
(assert ok? "a quoted set is a spec")
```

### `(attune! spec)`

`(attune! spec)` is the function's ceiling. It checks that the inferred signal
fits `spec`, and a body that may emit a signal outside `spec` is a compile
error. It checks and does not replace, so the function's signal stays the
inferred one, and a caller sees what the body does rather than what the
declaration admits. A body may declare more than one ceiling, and it must fit
each. A function that may fail but must not suspend declares
`(attune! :error)`.

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

(def sig (signal-of "(defn f [x] (attune! |:yield :error|) (+ x 1))" :f))
(assert (= |:error| (get sig :bits)) "the body raises, and does not yield")

(assert (string/contains? (compile-error '(fn [] (attune! :nosuch) 1))
                          "attune!: unknown signal keyword :nosuch"))
```

### `(silent!)`

`(silent!)` is `(attune! ||)`, and it is the one spelling of an empty ceiling.
A body that may emit anything, `:error` included, is a compile error. Generic
arithmetic may raise `:error` on a non-number, so `(silent!)` rejects it. A
body that passes on the signal of a parameter is rejected too.

```lisp
(defn select [flag a b]
  (silent!)
  (if flag a b))
(assert (= 1 (select true 1 2)))

(assert (string/contains?
          (compile-error '(fn [x y] (silent!) (+ x y)))
          "function restricted to {} but body may emit {:error}"))

(assert (string/contains?
          (compile-error '(fn [x y] (attune! ||) (+ x y)))
          "function restricted to {} but body may emit {:error}")
        "(attune! ||) is the same ceiling")
(assert (string/contains? (compile-error '(fn [x] (silent! x) x))
                          "silent! takes no arguments"))
```

### `(bound p spec)`

`(bound p spec)` bounds one parameter: the argument passed for `p` must emit
at most `spec`. The function no longer takes that parameter's signal from the
argument, so a higher-order function stops being polymorphic, and a call to
`p` contributes `spec`. `(bound p ||)` asks for a silent argument. The bound
is checked at function entry, and the check may raise, so the function
carries `:error`. Several `bound` forms may appear, one per parameter. A name
that is not a parameter is a compile error, and so is a second bound on one
parameter.

```lisp
(def sig (signal-of "(defn twice-silent [f x] (bound f ||) (f (f x)))"
                    :twice-silent))
(assert (= |:error| (get sig :bits)) "the entry check and the calls may raise")
(assert (empty? (get sig :propagates)) "twice-silent takes no signal from f")

(def sig (signal-of "(defn each-yield [f x] (bound f :yield) (f x))" :each-yield))
(assert (= |:yield :error| (get sig :bits)) "a call to f contributes the bound")

(assert (string/contains? (compile-error '(fn [x] (bound z ||) x))
                          "bound: 'z' is not a parameter of this function"))
(assert (string/contains? (compile-error '(fn [f] (bound f ||) (bound f :yield) f))
                          "bound: parameter f is already bounded"))
```

At entry, a closure is checked by its effective signal and a native function
by its declared signal. Any other value passes, and calling it raises inside
the function, which the `:error` covers. A value whose signal exceeds `spec`
fails the check with `:signal-violation`. So does a closure that passes on
the signal of its arguments: its bits are only a lower bound, so the check
cannot show that it fits.

```lisp
(defn apply-silent [f x]
  (bound f ||)
  (f x))

(assert (= 42 (apply-silent (fn [x] x) 42)))
(assert (= false (apply-silent callable? 42)) "a silent native passes")

(def opaque @[(fn [x] (yield x))])
(def [ok? err] (protect (apply-silent (get opaque 0) 42)))
(assert (= :signal-violation (get err :error)))
(assert (string/contains? (get err :message)
                          "bound: closure may emit {:yield} but parameter is bounded to {}"))

(def [ok? err] (protect (apply-silent (get @[length] 0) [1 2])))
(assert (string/contains? (get err :message)
                          "bound: length may emit {:error} but parameter is bounded to {}"))
```

When the compiler knows the argument's signal at the call, it checks the bound
there, and a violation is a compile error. It knows the signal of a lambda
literal, of a primitive, and of a binding whose signal it inferred. The entry
check stays for every other argument, such as the values read out of an array
above.

```lisp
(assert (string/contains?
          (compile-error '(begin (defn apply-silent [f x] (bound f ||) (f x))
                                 (apply-silent (fn [x] (yield x)) 42)))
          "may emit {:yield} but parameter f is bounded to {}"))

(assert (string/contains?
          (compile-error '(begin (defn apply-silent [f x] (bound f ||) (f x))
                                 (apply-silent length [1 2])))
          "may emit {:error} but parameter f is bounded to {}"))

(assert (string/contains?
          (compile-error '(begin (defn apply-silent [f x] (bound f ||) (f x))
                                 (defn call-it [k] (k))
                                 (apply-silent call-it 42)))
          "passes on the signal of its arguments but parameter f is bounded to {}"))
```

A violation nothing catches reports as any uncaught error does, at the call
that passed the value.

### Assertions

`(numeric!)` asserts that the function is GPU-eligible. The lowered body may
hold only numeric instructions and control flow: no call, no closure, no
heap value and no `emit`. The assertion also marks every parameter as a
number, which is the proof an intrinsic demands of its operands; see
[intrinsics.md](../intrinsics.md). Nothing checks the argument at run time,
so a non-number gives an unchecked result.

A call to a stdlib wrapper such as `+` becomes the wrapper's body
([grafting a call](../impl/graft.md)). Over a parameter the assertion marks,
the body's number guards fold away, and `(+ x 1)` leaves one `%add`. A call
that stays a call, such as `(println x)`, fails the assertion.

```lisp
(defn square [x] (numeric!) (%mul x x))
(assert (= 2.25 (square 1.5)))
(assert (get (protect (square "a")) 0) "a non-number argument raises nothing")

(defn inc1 [x] (numeric!) (+ x 1))
(assert (= 2 (inc1 1)) "+ grafts to one %add")

(assert (string/contains? (compile-error '(fn [x] (numeric!) (println x)))
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

`squelch` and `attune` are primitives, not declarations. They are the
boundary a caller puts around a closure it did not write. Each returns a new
closure that shares the original's bytecode and environment. When the new
closure returns a signal its mask covers, the boundary turns that signal into
a `:signal-violation` error.

Both take the spec first. `spec` is a keyword, a set, an array or list of
keywords, or an integer.

- `(squelch spec f)` blocks the signals in `spec` and lets the rest through.
- `(attune spec f)` lets through only the signals in `spec` and blocks the
  rest. `(squelch spec f)` is therefore `(attune (complement spec) f)`.

```lisp
(defn producer [] (yield 1))

(def [ok? err] (protect ((squelch :yield producer))))
(assert (= :signal-violation (get err :error)))
(assert (= "squelch: signal {:yield} caught at boundary" (get err :message)))

(def [ok? err] (protect ((attune |:yield :error| (fn [] (println "hi"))))))
(assert (= "squelch: signal {:io} caught at boundary" (get err :message)))

(def [ok? err] (protect (squelch producer :yield)))
(assert (= "squelch: second argument must be a closure, got keyword"
           (get err :message)))
```

Layers compose: a squelch of a squelched closure blocks both masks. The check
also fires when the squelched closure is called in tail position.

```lisp
(def quiet (squelch :io (squelch :yield (fn [] (println "hi")))))
(assert (= :signal-violation (get (get (protect (quiet)) 1) :error)))

(def blocked (squelch :yield producer))
(defn tail-call [] (blocked))
(assert (= :signal-violation (get (get (protect (tail-call)) 1) :error)))
```

Three classes of signal cross a `squelch` or `attune` boundary untouched.
`:error` and `:halt` pass whole, so squelching `:error` does nothing. The VM's
`:switch` trampoline passes. A signal that carries `:fuel` is a pause the VM
injects under code that is not silent, such as a fuel stop or a dynamic
breakpoint's `|:debug :fuel|`, and it passes whole too.
[signals/mod.rs](../../src/signals/mod.rs) holds the one predicate,
`squelched_bits`, that every tier asks.

```lisp
(def [ok? err] (protect ((squelch :error (fn [] (error {:error :mine}))))))
(assert (= :mine (get err :error)) "squelch never blocks :error")

(def paused (fiber/new (fn [] ((squelch :debug (fn [] (emit |:debug :fuel|) 5))))
                       |:debug :fuel :error|))
(fiber/resume paused)
(assert (= :paused (fiber/status paused)))
(assert (nil? (fiber/value paused)) "the pause passes a squelch of :debug as itself")
```

### Narrowing at compile time

When a `def` or `let` binds `(squelch spec f)` or `(attune spec f)` with a
literal spec, the analyzer computes the new closure's signal. It removes the
blocked bits, and adds `:error` when it removed any. A caller of the binding
then sees the narrower signal. A literal spec is a keyword or a set literal.
A set literal is a call to `set`, so where a binding shadows `set` the spec
is computed, and the binding is not narrowed.

```lisp
(def sig (signal-of "(defn p [] (yield 1))
(def safe (squelch :yield p))
(defn use [] (safe))" :use))
(assert (= |:error| (get sig :bits)))
```

A squelch that removes a bit adds `:error`, and `:error` cannot be squelched.
So a squelch never turns a closure that signals into a silent one.

## The boundary of a silent function

A function whose inferred signal is empty is a proof that nothing stops the
fiber while it runs. Backends depend on that proof. The MLIR and SPIR-V tiers
compile a silent function to code with no signal path at all, and the JIT
records no resume point inside one ([jit.md](jit.md)). Every tier therefore
keeps two rules: a meter never stops the fiber inside a silent function, and a
signal that leaves one is a broken proof.

A silent function spends fuel and never pauses. Each charge site inside it
takes one unit while the budget holds one, and takes nothing once the budget
is spent. The pause comes at the next charge site outside the silent
function, which finds the budget spent. So a fiber pays for the work a silent
function does, and a meter never stops the fiber inside one. The interpreter
and the JIT charge a silent function this way. The MLIR, SPIR-V and WASM
tiers charge nothing in the code they compile.

```lisp
(defn spin [n] (numeric!) (def @i 0) (while (%lt i n) (assign i (%add i 1))) i)
(assert (get (signal-of "(defn spin [n] (numeric!) (def @i 0)
  (while (%lt i n) (assign i (%add i 1))) i)" :spin) :silent))

(def metered (fiber/new (fn [] (spin 1000)) |:fuel|))
(fiber/set-fuel metered 5)
(fiber/resume metered)
(assert (= :dead (fiber/status metered)) "the meter never paused the silent loop")
(assert (= 1000 (fiber/value metered)))
(assert (= 0 (fiber/fuel metered)) "and the loop spent the budget")
```

A call through a mutable binding is a charge site the compiler cannot remove,
and a function that makes one is not silent. Here it is the first charge site
after the silent loop, so it finds the budget spent and pauses the fiber.

```lisp
(def @after identity)
(def outside (fiber/new (fn [] (after (spin 1000))) |:fuel|))
(fiber/set-fuel outside 5)
(fiber/resume outside)
(assert (= :paused (fiber/status outside)) "the call after the loop pauses")
(fiber/set-fuel outside 5)
(fiber/resume outside)
(assert (= 1000 (fiber/value outside)))
```

A signal that leaves a silent function is a broken proof: a defect in the
compiler, not in the program. It raises the terminal error
`{:error :broken-proof}`, whose bits are `:error` and `:terminal` together.
The message names the function and the signal it raised. `:error` gets no
exemption, because a caller depends on a silent function not raising. No mask
catches a terminal signal, so `protect`, `try` and `defer` never see it. It
ends the program with its report, and each fiber it passes stops where it
stood, so the chain stays inspectable.

Two signals pass a silent function's boundary as themselves. A halt passes,
because the depth cap halts a deep recursion: that is a resource fault, not a
broken proof. A signal that already carries `:terminal` passes whole, so the
report names the innermost function whose proof broke. A kill raises one: the
check at each charge site ends a killed worker inside a silent function as
outside one ([runtime](../runtime.md)).

Elle code cannot build a broken proof, because inference is sound.
[proof_tests.rs](../../src/vm/call/proof_tests.rs) forges a silent code
object that yields, and pins the report.

## Across files: signal projection

When a file returns a struct literal of closures, or a function whose body is
one, the compiler records a projection: a signal for each field. The
analyzer unwraps `begin`, `let`, `letrec` and `fn` bodies to find the struct,
and takes the union of the two branches of an `if`. Any other return shape
gives no projection.

An importing file that binds `((import "literal"))` looks the projection up.
Today the projected signal never reaches a call through `module:field`, so a
call into another file is treated as unknown (#1232). A `(silent!)` function
cannot call into another module until that is fixed.

## See also

- [Signal index](index.md)
- [Signals and JIT](jit.md)
- [Design philosophy](../philosophy.md) — why higher-order functions are
  polymorphic by default
