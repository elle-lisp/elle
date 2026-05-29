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

## Disposition table (eager trap at startup)

`elle::io::init_process_signals` runs from `main()` immediately after
`elle::config::init` and before `VM::new` (`src/main.rs`). It installs
process-wide POSIX traps before any worker thread spawns:

| Set | Signals | What we do |
|-----|---------|------------|
| Terminate | `TERM`, `INT`, `QUIT`, `HUP` | `sigaction(SA_RESTART)` to a handler that writes a tagged line to stderr (`elle: terminated by SIGTERM\n`, etc.) and calls `_exit(128 + signum)`. The handler is async-signal-safe — no Rust stdio, no allocation, no locks. Conventional shell exit codes: `143`, `130`, `131`, `129`. |
| Job control | `TSTP`, `TTIN`, `TTOU` | `sigaction` to a handler that calls `raise(SIGSTOP)`. The kernel stops the process; the shell can `bg`/`fg` it. On `SIGCONT` the process resumes mid-handler and returns normally. io_uring + signalfd state survive across SIGSTOP/SIGCONT untouched. |
| Resume | `CONT` | `sigaction` to an empty handler so the kernel has a delivery target. Nothing to clean up. |
| Pipe | `PIPE` | `sigaction(SIG_IGN)`. Writes to broken pipes surface as `EPIPE`; nothing entered the kernel queue. |
| Absorb | `USR1`, `USR2`, `CHLD`, `URG`, `WINCH`, `ALRM` | `pthread_sigmask(SIG_BLOCK)` on the main thread. With every worker also masking on spawn, no thread has these unblocked; the kernel queues them and nobody reads. Silently absorbed unless `os/sig-watch` opens a `signalfd` to drain. Accidental `kill -USR1 $pid` becomes a no-op. |
| Fault | `SEGV`, `BUS`, `FPE`, `ILL`, `ABRT`, `TRAP`, `SYS` | Untouched. Synchronous fault signals; intercepting them only obscures real bugs. |
| Uncatchable | `KILL`, `STOP` | Kernel forbids touching these. Pass through. |

## Watcher override

The terminate-set handlers are installed *process-wide* — but only
fire when the kernel can deliver the signal. With every worker thread
masking everything and `os/sig-watch :sigterm` adding SIGTERM to the
main thread's mask, **no thread has SIGTERM unblocked**. The kernel
parks the signal on the process pending queue, the sigaction handler
cannot fire, and the watcher's `signalfd` reads the event instead.
When the last watcher closes, the lazy-unblock on the main thread
re-enables kernel delivery, and the next SIGTERM runs the handler.

No explicit "watcher overrides built-in" coordination logic in user
space — the kernel's delivery rules do it for free. Signals in the
absorb set get a stricter version of the same trick: their close-time
unblock is suppressed (see "Mask policy" below), because there is no
sigaction handler to take over, and the kernel default for them is
Term — we'd be replacing a quietly-blocked queue with an active
process killer.

## Mask policy

1. **Worker threads block everything.** Every thread Elle spawns
   internally (the I/O thread pool, the stdin reader, the JIT worker,
   the user `(spawn closure)` worker) masks all maskable signals as
   its first action. Workers are never the kernel's chosen signal
   delivery target.
2. **The main thread blocks the absorb set at startup.** `USR1`,
   `USR2`, `CHLD`, `URG`, `WINCH`, `ALRM` are masked before `VM::new`
   runs. The terminate, job-control, resume, and pipe sets are
   *not* masked at startup — they are dispatched by the sigaction
   handlers installed alongside.
3. **`os/sig-watch` lazy-blocks the watched set on the main thread.**
   The first receiver for a signum bumps the refcount 0→1 and
   blocks the signal on the main thread (idempotent for absorb-set
   signums, which are already blocked).
4. **`os/sig-close` decrements the refcount.** When the last watcher
   for a signum closes, elle drains any instances still pending on
   the calling thread / process queue (via `sigwait`, which dequeues
   without invoking a handler). The unblock is then performed
   **only for signums outside the absorb set** — absorb-set signums
   stay masked so a subsequent `kill -USR1 $pid` doesn't escape via
   the kernel default (which is Term) onto a now-unblocked main
   thread. The drain still runs so a future watcher doesn't see
   stale pending state.
5. **macOS: per-receive worker unblock + no-op handler.**
   `init_process_signals` is currently Linux-shape on macOS too
   (sigaction installed, absorb set blocked on the main thread); the
   per-receiver kqueue path layers on top. Linux's
   `signalfd` reads pending signals directly from the kernel queue
   even when every thread blocks the signal, so the worker only
   needs to call `read(2)`. macOS's `EVFILT_SIGNAL` fires from the
   in-kernel signal-delivery path: when every thread blocks the
   signal the kernel parks it on the process pending list and the
   knote never activates. Elle works around this on macOS by
   installing a process-wide no-op `sigaction` handler for each
   newly-watched signal (refcounted; restored to the saved
   disposition when the last watcher closes), and by
   `pthread_sigmask`-unblocking the watched signals on the
   threadpool worker thread that calls `kevent()`. The kernel picks
   that worker as the delivery target, runs the no-op, and the
   knote activates so `kevent()` returns. None of this is visible to
   Lisp.

`(os/sig-mask)` always reflects the actual `pthread_sigmask` on the
calling thread — i.e. the main thread (and other elle-spawned
threads) after `os/sig-watch`. It does not reflect the per-worker
unblock used to feed `kevent()` on macOS.
`(os/sig-watching)` returns the set of signals currently held by at
least one receiver.

## Backend dispatch

Elle's async backend chooses how to wait on a `SignalReceiver`'s
kernel fd based on platform and CLI flags:

| Platform | Default | `--no-uring` |
|----------|---------|--------------|
| Linux | `IORING_OP_READ` on the signalfd, via the dedicated `submit_uring_sig_next` SQE helper. The read is queued on the io_uring instance and the kernel completes a CQE when one or more `signalfd_siginfo` records become available. No worker thread is involved on the elle side. | The threadpool worker calls `poll(POLLIN, -1)` and then `read(2)` on the signalfd. Uses one OS thread per outstanding `os/sig-next`. |
| macOS | n/a — io_uring is Linux-only | A threadpool worker calls `kevent()` on a per-receiver kqueue registered with `EVFILT_SIGNAL`. The worker temporarily `pthread_sigmask`-unblocks the watched signals on itself so the kernel can pick it as the delivery target (see "macOS: per-receive worker unblock + no-op handler" above). |

The threadpool path on Linux is exercised only when `--no-uring` is
passed on the CLI or `io_uring_setup(2)` fails on the host kernel
(extremely old / locked-down kernels). Production Linux always rides
the dedicated io_uring path.

## Documented limitations

These three are inherent to the chosen mechanism, not bugs.

### macOS does not report sender pid or uid

kqueue's `EVFILT_SIGNAL` reports the signal number and a coalesced
count, full stop. `:sender-pid` and `:sender-uid` are `nil` on macOS.
Programs that need sender identity — for example, SIGCHLD-driven
targeted child reaping — must call `subprocess/wait`-style logic to
recover it. Linux signalfd populates both fields.

### Library-spawned threads inherit the startup mask, not later watches

Elle masks all signals on every thread it spawns directly (threadpool,
stdin reader, JIT worker, user `(spawn closure)` worker). Threads
spawned by C dependencies (Cranelift codegen auxiliary threads,
libffi callbacks, future plugin cdylibs, anything called via FFI)
inherit whatever the *main thread's* mask was at *their* spawn time.

Under the eager-trap policy, the main thread's startup mask already
includes the absorb set (`USR1`, `USR2`, `CHLD`, `URG`, `WINCH`,
`ALRM`) — so a C-spawned thread post-`init_process_signals` has all
of those blocked, and an `os/sig-watch` on any of them
deterministically routes to the watcher's signalfd. The remaining
hole is the terminate/job-control/resume sets: a C-spawned thread has
them unblocked, and a `kill -TERM` could pick that thread to run the
sigaction handler (which still terminates correctly — just on the
"wrong" thread).

Practical impact is now minimal: the sigaction handler is identical
regardless of which thread runs it (`_exit(128 + signum)`). The
historical risk of a C-thread defeating `os/sig-watch` for a
controlled-disposition signal (USR1, etc.) is closed.

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
