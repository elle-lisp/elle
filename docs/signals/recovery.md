# Signal Recovery

## Non-Unwinding Recovery

The fiber model supports non-unwinding recovery without additional mechanism.
Recovery options emerge from the interaction of signals and resume.

### How it works

When a fiber signals, it **suspends** — its frames remain intact. The
parent fiber (or any ancestor in the chain) catches the signal and
decides what to do. If the signaling fiber advertised recovery options in its
payload, the handler picks one and resumes the child with that choice.
The child receives the resume value, dispatches on it, and continues.

Signals travel **up** the fiber chain (parent links). Recovery choices
travel back **down** the chain (resume calls). This is bidirectional
communication along the chain, not unwinding.

### Two recovery patterns

**Non-unwinding recovery**: The handler catches the signal,
inspects it, resumes the child with a recovery choice. The child
continues from where it suspended. The child's frames are never
discarded.

**Unwinding recovery** (via `try`/`catch`): The handler catches the signal and does
NOT resume the child. The child fiber becomes garbage. The handler
runs its own code in the parent fiber. This is a one-way trip.

Both patterns are just different uses of the same mechanism — resume vs.
don't resume. No special syntax or VM support is needed.

### Why this is strictly more powerful than traditional restart systems

- **Recovery options are data, not syntax.** The signal payload is a value, so
   available recovery options can be computed dynamically.
- **Multiple round-trips.** The handler resumes the child, the child
   signals again ("your suggestion also failed"), the handler tries
   something else. Arbitrary dialogue along the chain.
- **Composition through the chain.** If the immediate parent doesn't
   know what to do, it propagates the signal up the chain. An ancestor
   that understands the situation handles it and the recovery choice travels
   back down through resume calls.
- **The handler has full context.** It's running code in its own fiber
   with access to its own state. It can query databases, ask the user,
   or try multiple strategies before deciding.

### Example

```lisp
# The callee: signals with available recovery options
(defn safe-divide [a b]
  (if (= b 0)
    (emit :error
      {:error :division-by-zero
       :options [:use-value :return-zero]})
    (/ a b)))

# The handler: catches the signal, picks a recovery option
(defn compute []
  (let [[f (fiber/new (fn [] (safe-divide 10 0)) |:error|)]]
    (fiber/resume f nil)
    (if (= (fiber/status f) :suspended)
      # Child is suspended — we can resume it with a recovery choice
      (fiber/resume f {:option :use-value :value 1})
      (fiber/value f))))
```



## Error Signalling

Errors in Elle are signals — values emitted on the `:error` bit (bit 0,
`SIG_ERROR`). There is no exception hierarchy, no `Condition` type, no
`handler-case`. Error handling is fiber signal handling.

### Error Representation

The stdlib convention is a struct: `{:error :keyword :message "message"}`.

```lisp
# Stdlib primitive errors look like:
{:error :type-error :message "car: expected pair, got integer"}
{:error :division-by-zero :message "cannot divide by zero"}
{:error :arity-mismatch :message "expected 2 arguments, got 3"}
```

The `:error` keyword classifies the error. The `:message` string describes it.
Both are ordinary Elle values — no special types.

**This is a convention, not a hard rule.** Users can define their own error
value shapes. The signal system doesn't care what the payload is; it's just
a Value. Pattern matching on the payload is how handlers distinguish error
kinds. A user might prefer `[:boom "message"]`, or a plain string, or an
integer error code — whatever suits their domain.

### Two Failure Modes

**VM bugs** (stack underflow, bad bytecode, corrupted state): the compiler
emitted bad code or the VM has a defect. These panic immediately. Elle code
cannot catch them.

**Runtime errors** (type mismatch, arity error, division by zero, undefined
variable): program behavior on bad data. These are signalled via `SIG_ERROR`
and can be caught by a parent fiber with the appropriate mask.

### How Errors Flow

**From primitives**: All primitives are `NativeFn: fn(&[Value]) -> (SignalBits, Value)`.
Success returns `(SIG_OK, value)`. Error returns `(SIG_ERROR, error_struct)`.
The VM's dispatch checks signal bits after each primitive call.

**From instruction handlers**: Instructions like `Add`, `Car`, `Cdr` set
`fiber.signal` directly when they detect a type mismatch or other error.

**From Elle code**: Use `error` (a prelude macro) or `emit` directly:

```lisp
# Prelude macro — signals {:error :the-kw :message "..."} on SIG_ERROR
(try (error {:error :bad-input :message "expected a number"})
  (catch e (get e :error)))  # => :bad-input
```

### Catching Errors

Errors are caught by fibers whose mask includes the `:error` bit:

```lisp
# try/catch is sugar for fiber signal handling
(try
  (error {:error :test :message "boom"})
  (catch e
    (get e :error)))   # => :test
```

The `try`/`catch`, `protect`, `defer`, and `with` macros are all built on
fiber primitives. No special VM support.

### Error Propagation

Errors propagate up the fiber chain until caught:

1. Child signals `SIG_ERROR`
2. Parent checks: `child.mask & SIG_ERROR != 0`?
   - **Yes**: parent catches, child stays suspended (or becomes `error` state)
   - **No**: parent also suspends, signal propagates to grandparent
3. At the root fiber: uncaught error becomes `Err(String)` via the public API boundary

`fiber/propagate` re-signals a caught signal, preserving the child chain for
stack traces. `fiber/cancel` hard-kills a fiber (no unwinding).
`fiber/abort` injects an error and resumes a suspended fiber for graceful
unwinding (defer/protect blocks run).

### The Public API Boundary

`execute_bytecode` is the translation boundary between the signal-based
internal VM and the `Result<Value, String>` external API:

- `SIG_OK` → `Ok(value)`
- `SIG_ERROR` → `Err(format_error(signal_value))`

External callers (REPL, file execution, tests) see `Result`. Internal code
sees `SignalBits`.



## Migration Status

Steps 1–3 are complete. Steps 4–7 are future work.

1. ✅ **Fibers as execution context.** Fiber struct, FiberHandle,
   parent/child chain, signal mask, all fiber primitives implemented.
   Coroutines are fibers that yield.

2. ✅ **Unified signals.** Error and yield are signal types. Errors are
   `[:keyword "message"]` tuples.

3. ✅ **Signal-bits-based Signal type.** `Signal { bits: SignalBits,
   propagates: u32 }`. Inference tracks signal bits per function.

4. ❌ **Relax JIT restrictions.** JIT still restricted to silent functions.
   Signal-aware calling convention not yet implemented.

5. ❌ **User-defined signals.** Bit positions 16–31 reserved but no
   allocation API.

6. ✅ **Signal restrictions.** `silence` forms for signal contracts implemented.

7. ❌ **Erlang-style processes.** Fibers on an event loop with a scheduler.

---

## See also

- [Signal index](index.md)
