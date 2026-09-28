# Signal Protocol

<!-- audited: 2026-09-28 -->

How a signal is encoded, carried, caught and reported: the bits, the payload, propagation, I/O requests and the registry.

[inference.md](inference.md) owns what the compiler infers about signals and
the forms that bound it (`silence`, `muffle`, `squelch`).
[fibers.md](fibers.md) owns the fiber that carries a signal.

## Signal Types

Signal types are bit positions in a 64-bit bitfield (`SignalBits` wraps a
`u64`). The lower 32 bits are reserved for the runtime; the upper 32 bits are
available for user-defined signals. The runtime allocates bits 0 to 17:

| Bit | Name | Value | Meaning |
|-----|------|-------|---------|
| — | ok | 0 | Normal return (no bits set) |
| 0 | error | 1 | Error |
| 1 | yield | 2 | Cooperative suspension |
| 2 | debug | 4 | Breakpoint / trace |
| 3 | resume | 8 | VM-internal: fiber resume request |
| 4 | ffi | 16 | Calls foreign code |
| 5 | propagate | 32 | VM-internal: propagate caught signal |
| 6 | — | 64 | Unused |
| 7 | query | 128 | VM-internal: read VM state |
| 8 | halt | 256 | Graceful VM termination |
| 9 | io | 512 | I/O request to scheduler |
| 10 | terminal | 1024 | Uncatchable — passes through mask checks |
| 11–17 | runtime | — | Runtime signals and capability bits ([runtime.md](../runtime.md)) |
| 18–31 | reserved | — | Future runtime signals |
| 32–63 | user | — | User-defined signal types |

`abort`, the VM-internal graceful fiber termination, has no bit of its own. It
is `error` and `terminal` together, value 1025.

"ok" means no bits are set. A normal return has an empty signal bitfield.

The resume signal is how `fiber/resume` works — the primitive signals
the VM to perform the actual context switch.

## Signal Values

A signal carries a type (its bits) and a payload (an Elle value). The VM's
execution functions return the bits, and the payload waits on the fiber:

```rust
pub signal: Option<(SignalBits, Value)>
```

Empty bits mean a normal return, and the payload is the returned value. The
fast path is therefore one branch on the bits. Non-empty bits mean something
happened that may require handling.

**Signal payloads are arbitrary Values.** Any value can be an error payload,
a yield value, or a user-defined signal payload. There is no Condition type
or exception hierarchy. Pattern matching on the payload replaces hierarchy
checks — the handler inspects the signal value and dispatches accordingly.

## Signal Composition

**`SignalBits` is a pure bitmask.** Every bit is independent, any combination
of bits is valid, and the catcher decides what a combination means.

`intersects()` asks whether the two sets share **at least one** bit. It is
symmetric, and it is true for a partial overlap: `(A|B).intersects(B|C)` holds.
It is not a subset test. `SignalBits` has no subset test, because no caller
wants one — a mask catches a compound signal on any shared bit, and there is
no exception. `covers()` is the routing question a fiber mask asks, and it is
`intersects()` plus the empty-signal case; see
[capabilities.md](capabilities.md) for what that means for a mask.

Examples of composed signals:

- `|:yield|` — suspend, return a value to the caller
- `|:io|` — request I/O; the scheduler catches the bit and services the request
- `|:io :error|` — I/O error; a scheduler might log it and halt
- `|:yield :audit|` — suspend AND emit a user-defined audit signal; a
  monitoring fiber catches both

**Fiber masks work the same way.** A fiber mask like `|:yield :io|` catches
fibers that have either bit set. The mask is a bitmask, not an enum.

**User-defined signals (bits 32–63) compose freely** with built-in bits. A
user-defined signal can be combined with `:yield`, `:error`, `:io`, or any
other bit.

## Terminal vs Resumable Signals

A signal carrying `:terminal` passes every mask, and `:halt` cannot be
resumed. For every other signal, ending the child or resuming it is the
handler's decision, not a property of the signal. The handler catches the
signal and either resumes the child or does not, so the same signal can be
resumable in one context and terminal in another.

## Propagation

Signal propagation (Janet model):

1. The child fiber emits a signal: it stores the value in `child.signal`, and
   it stops.
2. The VM hands the signal bits to the parent's `fiber/resume`.
3. The parent asks whether the child's mask covers the bits. The child's mask
   records which signals the parent catches from it.
    - **Caught**: Parent handles the signal. Child is `:paused` and
      reachable via parent's `child` pointer.
    - **Not caught**: the signal passes the parent's `fiber/resume` call,
      and the parent stops there too. An error leaves the child `:error`.
      The signal propagates up until caught or reaches root.
4. Handler walks `child` chain to find originator. Every fiber in the
   chain is stopped and inspectable via `fiber/value`.

This is O(1) dispatch — a single AND operation. No handler chain traversal.
When a handler catches a signal, it can walk the fiber chain to inspect the
propagation path. Every fiber in the chain is stopped and can be resumed
independently for non-unwinding recovery ([recovery.md](recovery.md)).

## Reaching the root

The root of the program is where propagation stops. `:error` and `:halt`
have answers there: the error prints with the location of the form that
raised it, and the halt ends the program with its value. Every other
signal reaches the root with no handler left to run, so the program stops
and reports which signal went unhandled:

```text
Unhandled signal {:io} outside fiber context
```

The report names the emitted bits through the signal registry. A raw mask
names no signal: the reader would have to know which bit position each
primitive raises. `:io` at the root means the program ran no scheduler. A
user-defined keyword at the root means no fiber masked it.

One message answers for every bit. Nothing caught the signal, and that is
the whole condition, so `:yield` gets no report of its own.

## I/O Signals

### I/O and the Scheduler

I/O signals use the `:io` bit (bit 9). Every port, socket and file primitive
that reaches the scheduler has the signal `Signal::io_yields_errors()`, which
is `:io` and `:error`. `may_io()` asks whether a signal includes `:io`.

### Stream Primitives and I/O Requests

Stream primitives (`port/read-line`, `port/read`, `port/read-all`,
`port/write`, `port/flush`) do not perform I/O themselves. Instead, they:

1. Build an `IoRequest` (typed descriptor of the I/O operation)
2. Return `(|:io|, request)` to raise the request
3. Let the scheduler catch the fiber on the `:io` bit and dispatch the
   `IoRequest` payload to a backend

The backend (`AsyncBackend`) performs the actual I/O and answers a result or
an error. The scheduler resumes the fiber with the result.

### `:io` does not imply `:yield`

An I/O request raises `|:io|` and nothing else. It does suspend the calling
fiber — but so does every signal. Suspension follows from raising a signal at
all, not from any particular bit: `signals::dispatch::is_suspending` parks on
anything that is neither empty, nor an error, nor a halt, and the VM and the
WASM tier both ask it.

So `:yield` means one thing: the cooperative suspension `(yield v)` raises and
a `|:yield|` mask catches. A request that carried it too would be caught by
every generator's mask on its way to the scheduler:

- `|:io|` — an I/O request
- `|:yield|` — a generator yielding a value
- `|:io :error|` — an I/O error; a scheduler might log it and halt
- `|:io :audit|` — an I/O request that also emits an audit signal

This is what lets a generator masked `|:yield|` do I/O in its body:
`port/lines` ([stdlib.lisp](../../src/stdlib.lisp)), `tls/lines`
([tls.lisp](../../lib/tls.lisp)), and the SSE streams in
[sse.lisp](../../lib/http/sse.lisp) all have that shape. Their `(yield line)`
is caught by the consumer; their `port/read-line` raises `|:io|`, which the
mask does not name, so it travels out to the scheduler. Pinned by
[io-request-carries-no-yield.lisp](../../tests/lang/io-request-carries-no-yield.lisp).

## Signal Registry

The signal registry maps signal keywords to bit positions. Built-in signals
occupy bits 0–17; bits 18–31 are runtime-reserved; user-defined signals use
bits 32–63.

### Built-in Signals

| Keyword | Bit | Meaning |
|---------|-----|---------|
| `:error` | 0 | Error signal |
| `:yield` | 1 | Cooperative suspension |
| `:debug` | 2 | Breakpoint/trace |
| `:ffi` | 4 | Calls foreign code |
| `:halt` | 8 | Graceful VM termination |
| `:io` | 9 | I/O request to scheduler |
| `:exec` | 11 | Subprocess capability |
| `:fuel` | 12 | Instruction budget exhaustion |
| `:wait` | 14 | Structured concurrency wait request |
| `:gpu` | 15 | GPU dispatch capability |
| `:os-signal` | 16 | POSIX signal send and raise capability |
| `:fs` | 17 | Filesystem capability |

Bits 3, 5, 7, 10 and 13 are VM-internal and carry no keyword: resume,
propagate, query, terminal and the fiber-switch trampoline. Bit 6 is unused.
[runtime.md](../runtime.md) owns the runtime bits.

```lisp
(assert (= (get (signals) :fs) 17) "the registry names the filesystem bit")
```

### User-Defined Signals

User-defined signals are registered via the `(signal :keyword)` special
form and allocated bits 32–63 sequentially. Up to 32 user signals are
supported. Registration happens at analysis time.

```lisp
(signal :heartbeat)
(signal :rate-limit)
# :heartbeat gets bit 32, :rate-limit gets bit 33

# Expression position — returns the keyword
(def my-signal (signal :custom))
my-signal  # => :custom
```

Duplicate registration is a compile-time error. Built-in signal keywords
cannot be re-registered.

---

## See also

- [Signal index](index.md)
