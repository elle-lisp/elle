# emit

`emit` is the single mechanism for all signal emission in Elle. Every
signal — yields, errors, I/O requests, user-defined signals — goes
through `emit`.

## Syntax

```text
(emit :keyword value)       # emit a single signal
(emit |:kw1 :kw2| value)    # emit compound signal bits
(emit :keyword)             # emit with nil payload
```

The first argument must be a literal keyword or keyword set. The
compiler extracts the signal bits at compile time. The second argument
is the payload (defaults to nil).

## Common signals

```text
(emit :yield 42)            # cooperative suspension, payload 42
(emit :error {:error :type-error :message "boom"})  # error signal
(emit :io request)          # I/O request to scheduler
(emit |:yield :io| data)    # compound: yield + I/O
```

## yield and error are macros

`yield` and `error` are prelude macros that expand to `emit`:

```text
(yield 42)     # => (emit :yield 42)
(yield)        # => (emit :yield nil)
(error "boom") # => (emit :error "boom")
(error)        # => (emit :error nil)
```

There is nothing special about `:yield` or `:error` as signal keywords.
They are ordinary entries in the signal registry. `emit` treats all
keywords uniformly.

## Fiber masks catch emitted signals

When a fiber emits a signal, the parent catches it if the signal bits
overlap the fiber's mask:

```text
(let ([f (fiber/new (fn [] (emit :yield 42)) |:yield|)])
  (fiber/resume f))   # => 42

(let ([f (fiber/new (fn [] (emit :yield 42)) 0)])
  (fiber/resume f))   # signal propagates (mask doesn't catch :yield)
```

## Suspension vs error

`emit` distinguishes two behaviors based on the signal bits:

- **Error signals** (bits containing `:error`): the fiber stops and the
  error propagates through the call stack. The fiber is not resumable
  from the emit point. This is how `(error val)` works.

- **Suspension signals** (everything else: `:yield`, `:io`, user-defined):
  the fiber suspends with a `SuspendedFrame`. The parent can resume the
  fiber, and the resume value becomes the result of the `(emit ...)`
  expression.

```text
# Suspension: emit :yield, get resume value back
(let ([x (emit :yield :waiting)])
  # x is whatever the parent passes to fiber/resume
  (+ x 1))

# Error: emit :error, no resume
(emit :error {:error :oops :message "something went wrong"})
# control never returns here
```

## User-defined signals

Any keyword can be a signal. Register it with `signal/register` and
use it with `emit`:

```text
(signal/register :heartbeat)
(emit :heartbeat {:timestamp (clock/monotonic)})
```

The parent catches it through the mask like any other signal:

```text
(fiber/new body |:heartbeat|)
```

## Dynamic emit (primitive fallback)

When the first argument is not a literal keyword or set, `emit` falls
through to the runtime primitive. This supports dynamic signal selection:

```text
(emit some-variable value)   # runtime dispatch, not compile-time
```

This is rare. Prefer literal keywords for compile-time signal inference.

## Capability interaction

If a fiber has denied capabilities (via `:deny` on `fiber/new`), the
`emit` instruction itself is not affected — `emit` is a bytecode
instruction, not a primitive call. A fiber with `:deny |:error|` can
still `(emit :yield val)`. Capability enforcement applies to primitive
calls, not to `emit`.

See [capabilities.md](capabilities.md) for the full capability system.
