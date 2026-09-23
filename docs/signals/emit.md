# emit

<!-- audited: 2026-09-23 -->

`emit` raises a signal from Elle code: a yield, an error, or a signal the program declares.

A primitive raises its signals from Rust instead. An I/O primitive returns
`:io` with its request, and a failing primitive returns `:error`. The fiber
that receives a signal sees the same thing either way.

## Syntax

`(emit signal value)` raises `signal` with `value` as its payload, and
`(emit signal)` raises it with a nil payload. `signal` is a keyword or a
keyword set such as `|:yield :io|`. When it is a literal, the compiler
resolves its bits at compile time and emits the `Emit` instruction:

```lisp
(let [f (fiber/new (fn [] (emit :yield 42)) |:yield|)]
  (assert (= (fiber/resume f) 42))         # resume answers the payload
  (assert (= (fiber/value f) 42)))

(let [f (fiber/new (fn [] (emit :yield)) |:yield|)]
  (fiber/resume f)
  (assert (nil? (fiber/value f))))         # no payload is nil

(let [f (fiber/new (fn [] (emit |:yield :io| :both)) |:yield|)]
  (fiber/resume f)
  (assert (= (fiber/value f) :both)))      # a mask catches a set it overlaps
```

A literal keyword the registry does not know is a compile error:

```lisp
(let [[ok? err] (protect (compile/whole-module "(emit :nosuch 1)" "<doc>"))]
  (assert (not ok?))
  (assert (= (get err :error) :compile-error)))
```

## yield and error are macros

`yield` and `error` are prelude macros that expand to `emit`:

```lisp
(assert (= (expand-macro '(yield 42)) '(emit :yield 42)))
(assert (= (expand-macro '(yield)) '(emit :yield nil)))
(assert (= (expand-macro '(error "boom")) '(emit :error "boom")))
(assert (= (expand-macro '(error)) '(emit :error nil)))
```

There is nothing special about `:yield` or `:error` as signal keywords.
They are ordinary entries in the signal registry. `emit` treats all
keywords uniformly.

## Fiber masks catch emitted signals

When a fiber emits a signal, the parent catches it if the signal bits
overlap the fiber's mask. Otherwise the signal passes through the parent's
`fiber/resume` to the next fiber up:

```lisp
(let [outer (fiber/new
              (fn []
                (let [inner (fiber/new (fn [] (emit :yield 42)) 0)]
                  (fiber/resume inner)       # a mask of 0 catches nothing
                  :inner-returned))
              |:yield|)]
  (assert (= (fiber/resume outer) 42)))     # the yield reached outer's parent
```

## Suspension and errors

Every signal a parent catches leaves the child `:paused`, with its frames kept
as a `SuspendedFrame`. The parent can resume the child, and the resume value
becomes the result of the `(emit ...)` expression:

```lisp
(let [f (fiber/new (fn [] (+ 1 (emit :yield :waiting))) |:yield|)]
  (assert (= (fiber/resume f) :waiting))
  (assert (= (fiber/resume f 10) 11))       # the resume value is emit's result
  (assert (= (fiber/status f) :dead)))
```

An `:error` is no exception. The fiber that caught it decides whether to
resume the child with a recovery value or to leave it; `try` and `protect`
leave it. [primitives.md](primitives.md) states the rule.

```lisp
(let [f (fiber/new (fn [] (emit :error {:error :oops :message "recoverable"})) |:error|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused))     # an error parks the fiber too
  (assert (= (fiber/resume f :recovered) :recovered)))
```

## User-defined signals

Any keyword can be a signal. Declare it with `(signal :keyword)`, emit it,
and catch it through the mask like any other signal:

```lisp
(signal :heartbeat)

(let [f (fiber/new (fn [] (emit :heartbeat {:beat 1}) :done) |:heartbeat|)]
  (assert (= (fiber/resume f) {:beat 1}))
  (assert (= (fiber/resume f) :done)))
```

A user signal is allocated a bit in the 32–63 range
([protocol.md](protocol.md)), and the literal `emit` above carries that
bit into the bytecode whole — a user signal suspends, routes, and gets
squelched exactly as `:yield` does. The `Emit` operand is 64 bits wide
for this reason ([impl/bytecode.md](../impl/bytecode.md)).

## Dynamic emit (primitive fallback)

When the first argument is not a literal keyword or set, `emit` compiles to
a call to the `fiber/emit` primitive, which resolves the bits at run time:

```lisp
(def which :heartbeat)

(let [f (fiber/new (fn [] (+ 1 (emit which 2))) |:heartbeat|)]
  (assert (= (fiber/resume f) 2))
  (assert (= (fiber/resume f 5) 6)))        # the resume value is the call's result
```

The dynamic form suspends, routes, and resumes exactly as the literal one
does, and the resume value is its result the way a return value is any
call's result. What differs is the shape of the compiled code: the literal
`emit` ends a basic block and resumes in the next one, while the dynamic
`emit` is an ordinary call the fiber parks inside of.

This is rare. Prefer literal keywords for compile-time signal inference.

## Capability interaction

A literal `emit` is a bytecode instruction, not a primitive call, so its
handler never reads the fiber's withheld set. A fiber raises any bit it
names that way, including one it withholds. A dynamic `emit` is a call to
`fiber/emit`, and the capability gate reads the bits its first argument
names ([authority.md](authority.md)):

```lisp
(let [f (fiber/new (fn [] (emit :heartbeat :literal))
                   |:heartbeat| :deny |:heartbeat|)]
  (assert (= (fiber/resume f) :literal)))   # the literal form is not gated

(let [f (fiber/new (fn [] (emit which :dynamic))
                   |:heartbeat :error| :deny |:heartbeat|)]
  (assert (= (get (fiber/resume f) :error) :capability-denied)))
```

See [capabilities.md](capabilities.md) for the full capability system.
