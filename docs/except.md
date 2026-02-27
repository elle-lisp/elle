# Error Handling

Errors in Elle are signals propagating through fibers. There is no exception
hierarchy, no `Condition` type, no `handler-case`. This document describes
the implemented error model.


## Error Representation

Errors are tuples: `[:keyword "message"]`.

```rust
// Construct an error value
error_val("type-error", "car: expected pair, got integer")
// → [:type-error "car: expected pair, got integer"]

// Extract human-readable message
format_error(value)
// → "type-error: car: expected pair, got integer"
```

The keyword classifies the error. The string describes it. Both are
ordinary Elle values — no special types.


## Two Failure Modes

**VM bugs** (stack underflow, bad bytecode, corrupted state): the compiler
emitted bad code or the VM has a defect. These panic immediately. Elle code
cannot catch them.

**Runtime errors** (type mismatch, arity error, division by zero, undefined
variable): program behavior on bad data. These are signaled via `SIG_ERROR`
and can be caught by a parent fiber with the appropriate mask.


## How Errors Flow

### From primitives

All primitives are `NativeFn: fn(&[Value]) -> (SignalBits, Value)`.

```rust
// Success
(SIG_OK, Value::int(42))

// Error
(SIG_ERROR, error_val("type-error", "car: expected pair"))
```

The VM's `handle_primitive_signal()` dispatches by signal bits:
- `SIG_OK` → push value to stack
- `SIG_ERROR` → store in `fiber.signal`, push NIL to keep stack consistent

### From instruction handlers

Instruction handlers (Add, Sub, Car, Cdr, etc.) set `fiber.signal` directly:

```rust
self.fiber.signal = Some((SIG_ERROR, error_val("type-error", msg)));
self.fiber.stack.push(Value::NIL)# // keep stack consistent
```

The dispatch loop checks `fiber.signal` after each instruction and returns
immediately on `SIG_ERROR`.

### From Elle code

```lisp
(fiber/signal 1 [:division-by-zero "cannot divide by zero"])
```

`fiber/signal` with bit 0 (`SIG_ERROR`) emits an error signal. The fiber
suspends and the signal propagates up the chain.


## Signal-Based Error Handling

Error handling is fiber signal handling. The pattern:

1. Create a child fiber with `SIG_ERROR` in its mask
2. Resume the child
3. Check the signal bits:
   - `SIG_OK` (0): child completed normally, read `fiber/value`
   - `SIG_ERROR` (1): child errored, read `fiber/value` for the error tuple

```lisp
;# Manual error handling (try macro will sugar this)
(let ((f (fiber/new (fn () (/ 1 0)) 1)))  # mask = SIG_ERROR
  (fiber/resume f nil)
  (if (= (fiber/status f) :dead)
    (fiber/value f)                        # normal result
    (begin
      (println "caught:" (fiber/value f))  # error tuple
      :recovered)))
```

### Non-unwinding recovery

Because the child fiber is suspended (not unwound), the handler can resume
it with a recovery value:

```lisp
(let ((f (fiber/new (fn ()
           (let ((x (fiber/signal 1 [:need-value "provide a default"])))
             (* x 2)))
         1)))
  (fiber/resume f nil)          # child signals, suspends
  (fiber/resume f 21))          # resume with recovery value → 42
```

The resume value is pushed onto the child's operand stack. Execution
continues as if the signal expression evaluated to that value.


## The Public Boundary

`execute_bytecode` is the translation boundary between the signal-based
internal VM and the `Result<Value, String>` external API:

- `SIG_OK` → `Ok(value)`
- `SIG_ERROR` → `Err(format_error(signal_value))`

External callers (REPL, file execution, tests) see `Result`. Internal
code sees `SignalBits`.


## Error Propagation

Errors propagate up the fiber chain until caught:

1. Child signals `SIG_ERROR`
2. Parent checks: `child.mask & SIG_ERROR != 0`?
   - **Yes**: parent catches, child stays suspended
   - **No**: parent also suspends, signal propagates to grandparent
3. At the root fiber: uncaught error becomes `Err(String)` via
   `format_error`

The `fiber/propagate` primitive re-raises a caught signal, preserving the
child chain for stack traces.

The `fiber/cancel` primitive injects an error into a suspended fiber,
transitioning it to `Error` status.


## try/catch (Future)

`try`/`catch` will be a macro over fiber primitives:

```lisp
(try
  (risky-operation)
  (catch e
    (handle-error e)))

;# Expands to approximately:
(let ((f (fiber/new (fn () (risky-operation)) 1)))
  (fiber/resume f nil)
  (if (= (fiber/status f) :error)
    (let ((e (fiber/value f)))
      (handle-error e))
    (fiber/value f)))
```

This is blocked on the macro system. The fiber primitives work today;
the sugar is what's missing.


## What Was Removed

The following have been deleted entirely:

- `Condition` type and `condition.rs`
- `Exception` struct and exception hierarchy
- `handler-case` / `handler-bind` / `unwind-protect` special forms
- `PushHandler` / `PopHandler` / `CheckException` / `MatchException` /
  `ClearException` / `ReraiseException` / `BindException` / `LoadException`
  bytecode instructions
- `current_exception` field on VM/Fiber
- `exception_handlers` stack
- `VmAwareFn` type
- `LError` / `ErrorKind` types
- `ResumeOp` on coroutines

Errors are just values. Signal handling is just fiber masks.


## JIT Error Handling

JIT-compiled functions check for pending errors after every function call.
If `fiber.signal` is set after a call returns, the JIT bails out to the
interpreter. Functions containing exception handling instructions are not
JIT-compiled.

Future work: JIT-native signal checks (check a flag instead of calling a
runtime helper).
