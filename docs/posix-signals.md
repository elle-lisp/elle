# POSIX signals

Elle programs can send POSIX signals to other processes and observe
signals delivered to themselves. The surface lives under `os/sig-*`.

The word "signal" is overloaded:

- **Elle signals** (`:yield`, `:io`, `:error`, …) — the runtime's
  unified control-flow mechanism documented in
  [`signals/`](signals/). They are *compile-time inferred* and flow
  up the fiber chain.
- **POSIX signals** (`SIGTERM`, `SIGINT`, `SIGUSR1`, …) — kernel
  notifications delivered to the process. Documented here. Use the
  `os/sig-*` primitives; never the bare word "signal".

## Quick start

Watch for `SIGTERM` and `SIGINT`, log each delivery, exit on first:

```text
(def r (os/sig-watch |:sigterm :sigint|))
(each ev (os/sig-next r)
  (println :received (get ev :signal) :from (get ev :sender-pid)))
(os/sig-close r)
(sys/exit 0)
```

Send `SIGUSR1` to another process:

```text
(os/sig-send 4242 :sigusr1)
```

Send a signal to yourself:

```text
(os/sig-raise :sigusr1)
```

## Surface

| Primitive | Capability | Notes |
|-----------|-----------|-------|
| `(os/sig-send pid sig)` | `:os-signal` | `kill(2)`. `sig` is a keyword (`:sigterm`) or a named integer (`15`). |
| `(os/sig-raise sig)` | `:os-signal` | `raise(3)`. Equivalent to `(os/sig-send (sys/pid) sig)`. |
| `(os/sig-watch sig-set)` | — | Blocks the signals on the calling thread and returns a `SignalReceiver`. Yields `:io` only on `os/sig-next`. |
| `(os/sig-next receiver)` | — | Yields `SIG_YIELD\|SIG_IO`. Resumes with an array of delivered-signal structs. |
| `(os/sig-close receiver)` | — | Closes the receiver. When the last receiver for a signal closes, the signal is unblocked. Idempotent. |
| `(os/sig-pending)` | — | Returns a set of keywords for signals currently pending delivery on this thread (`sigpending(2)`). |
| `(os/sig-mask)` | — | Returns a set of keywords for signals currently blocked on this thread (`pthread_sigmask`). |
| `(os/sig-watching)` | — | Returns a set of keywords for signals currently being watched by at least one live receiver. |

## Recognised signals

`os/sig-*` accepts the standard set:

`:sigterm` `:sigkill` `:sighup` `:sigint` `:sigquit` `:sigpipe`
`:sigalrm` `:sigusr1` `:sigusr2` `:sigchld` `:sigcont` `:sigstop`
`:sigtstp` `:sigttin` `:sigttou` `:sigwinch`

Integer signums are accepted only if they round-trip to one of the
above names. Unknown integers (including realtime signals
`SIGRTMIN..SIGRTMAX`) are an `:argument-error`. Realtime signals are
not exposed in v1.

`:sigkill` and `:sigstop` can be *sent*; they cannot be watched (the
kernel forbids blocking them).

## Delivered-signal struct

`(os/sig-next receiver)` resolves to an *array* of structs (signalfd
batches deliveries; the array is often one element but may be
several):

```text
[{:signal :sigterm
  :sender-pid 1234
  :sender-uid 1000
  :code 0
  :count 1}]
```

| Field | Type | Meaning |
|-------|------|---------|
| `:signal` | keyword | The delivered signal. |
| `:sender-pid` | int or nil | The sender's pid. `nil` on macOS. |
| `:sender-uid` | int or nil | The sender's uid. `nil` on macOS. |
| `:code` | int | `siginfo.ssi_code` (`SI_USER=0`, `SI_KERNEL=128`, `CLD_EXITED=1`, …). `0` on macOS. |
| `:count` | int | Always `1` on Linux. On macOS the kevent coalesced count. |

## Mask policy

POSIX signalfd and kqueue both require the target signal to be
blocked process-wide first — otherwise the default disposition fires
in some thread before the watcher reads it. Elle handles the mask
automatically. The policy:

1. **Worker threads block everything.** Every thread Elle spawns
   internally (the I/O thread pool, the stdin reader, etc.) masks
   all maskable signals as its first action. Workers are never the
   kernel's chosen signal delivery target.
2. **The main thread blocks lazily.** Before the first
   `os/sig-watch` call, default disposition is in force for every
   signal — `kill -TERM $pid` terminates an elle program just like
   any other Unix program. The first `os/sig-watch` atomically blocks
   the requested signals on the main thread and opens the
   signalfd / kqueue.
3. **Refcounted block; unblock when refcount hits zero.** A signal
   stays blocked as long as at least one receiver watches it.
   `os/sig-close` decrements; when the last watcher for a signal
   closes, the signal is unblocked. Any instance pending in the
   kernel queue at the moment of unblock fires its default
   disposition immediately — this matches plain POSIX semantics and
   is preferred to silently swallowing the event.

`(os/sig-mask)` always reflects the actual `pthread_sigmask` on the
calling thread. `(os/sig-watching)` returns the set of signals
currently held by at least one receiver.

## Documented limitations

These three are inherent to the chosen mechanism, not bugs.

### macOS does not report sender pid or uid

kqueue's `EVFILT_SIGNAL` reports the signal number and a coalesced
count, full stop. `:sender-pid` and `:sender-uid` are `nil` on macOS.
Programs that need sender identity — for example, SIGCHLD-driven
targeted child reaping — must call `subprocess/wait`-style logic to
recover it. Linux signalfd populates both fields.

### Library-spawned threads can defeat the worker-mask invariant

Elle masks all signals on every thread it spawns directly. Threads
spawned by C dependencies (the cranelift JIT codegen threads, libffi
callbacks, future plugin cdylibs, anything called via FFI) inherit
whatever the *main thread's* mask was at *their* spawn time. If you
call `os/sig-watch` after such a thread has spawned, the kernel may
still select that thread as the delivery target and fire the default
disposition.

Practical mitigation: call `os/sig-watch` early in your program,
before triggering JIT compilation, before loading FFI plugins. There
is no portable way to retroactively change another thread's signal
mask.

### Standard signals coalesce

Two `SIGUSR1` deliveries from the same sender between consecutive
`os/sig-next` reads collapse to one event in the kernel queue. This
is plain POSIX behaviour for non-realtime signals; it is not
elle-specific. `:count` on each event is a best-effort approximation,
not a reliable counter. Do not use signals as a counting channel.

## REPL and Ctrl-C

The REPL uses `rustyline`, which installs its own SIGINT handler so
Ctrl-C cancels the current input. If you call
`(os/sig-watch |:sigint|)` in the REPL, that handler stops seeing
Ctrl-C — the watcher takes ownership. Ctrl-\\ (SIGQUIT) is unaffected.

This is the intended behaviour. If you want a quiet REPL again,
`(os/sig-close r)` releases the lease.

## Cancel semantics

A fiber cancelled while parked in `os/sig-next` follows the same path
as a fiber cancelled in `watch-next`:

- On the io_uring backend (Linux), the underlying read is cancelled
  with `IORING_OP_ASYNC_CANCEL`; the receiver survives and can be
  reused.
- On the thread-pool backend (macOS, CI, older Linux), there is no
  `shutdown(2)` equivalent to wake a blocking `read()` on signalfd,
  so cancel closes the receiver. The next `os/sig-next` on it will
  fail. Wrap with `(ev/race timer (os/sig-next r))` and you must
  re-create the receiver on each timeout.

## Patterns

### Graceful shutdown

```text
(def r (os/sig-watch |:sigterm :sigint|))
(ev/spawn (fn []
  (each ev (os/sig-next r)
    (eprintln "shutting down on " (get ev :signal))
    (cleanup!)
    (sys/exit 0))))
```

### Reload-on-SIGHUP

```text
(def r (os/sig-watch |:sighup|))
(forever
  (each _ (os/sig-next r) (reload-config!)))
```

### Conditional disposition

```text
# Block SIGPIPE for the duration of a write loop so a closed peer
# manifests as a write error instead of process termination.
(def r (os/sig-watch |:sigpipe|))
(defer (os/sig-close r)
  (write-loop!))
```

## Capabilities

`os/sig-send` and `os/sig-raise` carry the `:os-signal` capability
bit. A fiber created with `(fiber/new body :deny |:os-signal|)`
cannot send signals — the call emits a `:capability-denied` signal
that the parent fiber catches.

`:os-signal` is distinct from `:exec`. Denying `:exec` blocks
`subprocess/exec` and `subprocess/kill` (since the latter is sending a
signal to *spawned* children); denying `:os-signal` blocks generic
signal sends to arbitrary pids. Either may be denied independently.

`os/sig-watch`, `os/sig-next`, `os/sig-close`, `os/sig-pending`,
`os/sig-mask`, and `os/sig-watching` do not carry a capability bit —
they observe process state without sending.

## See also

- [`signals/`](signals/) — Elle's runtime signal system (different
  concept, different word).
- [`io.md`](io.md) — async scheduler that `os/sig-next` integrates
  with.
- [`subprocess`](io.md#subprocesses) — `subprocess/kill` for sending
  a signal to a child handle.
