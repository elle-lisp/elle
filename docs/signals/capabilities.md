# Capability enforcement

Capabilities flow down. A fiber's parent decides what the fiber is
permitted to do. Operations the fiber can't perform become signals the
parent can catch.

## Creating a restricted fiber

`fiber/new` accepts `:deny` after the mask argument:

```text
# Deny IO — child can't call IO primitives
(fiber/new body |:io :error| :deny |:io|)

# Deny IO and FFI
(fiber/new body |:io :ffi :error| :deny |:io :ffi|)

# No restrictions (default — unchanged existing API)
(fiber/new body |:error|)
```

The mask (second argument) controls signal routing — what the parent
catches. The `:deny` keyword controls enforcement — what the child
can't do. They're independent:

| | mask has `:io` | mask lacks `:io` |
|---|---|---|
| **deny has `:io`** | blocked; parent catches denial | blocked; denial propagates |
| **deny lacks `:io`** | child does IO; parent catches signal | child does IO silently |

The table reads the same for every capability. A mask catches a signal
when the two share **any** bit, so naming a capability in the mask catches
the operations that raise it — `:exec` catches subprocess requests, `:fs`
catches the filesystem denials, `:io` catches scheduler requests. There is
no bit a mask must name before another bit takes effect.

Naming a capability catches the operation whether or not you also deny it.
Denial changes what the child is *allowed* to do; the mask changes what
you *see*:

```lisp
# Watch subprocess calls without forbidding them.
(let [f (fiber/new (fn [] (subprocess/system "echo" ["hi"]) :done)
                   |:error :exec|)]
  (fiber/resume f)          # => #<io-request>, the fiber is :paused
  (fiber/bits f))           # => 2560 = |:io :exec|
```

Catching a request makes you responsible for it. The fiber is parked until
you service the request — with `io/submit` against a backend, or by
re-raising it so your own scheduler handles it — and resume the child with
the result. A mask you do not intend to service should not name the bit.

## Deniable capabilities

Every signal bit is a capability bit. A fiber's `:deny` set is tested
against the bits a primitive *declares*, so any bit a primitive declares
is one a parent can withhold. Some bits also drive dispatch — `:io`
routes a request through the scheduler — and some do no dispatch work at
all. That distinction belongs to the VM. It changes neither what a mask
can deny nor what it can catch.

`:yield` is not in this table. It is the cooperative suspension `(yield v)`
raises, not a capability: an I/O request raises `|:io|` alone, so a
generator masked `|:yield|` catches its own yields while its reads travel
out to the scheduler. See `docs/signals/protocol.md`.

| Keyword | Bit | Effect when denied | Dispatch |
|---------|-----|--------------------|----------|
| `:error` | 0 | Blocks primitives that may error (~66% of all) | yes |
| `:yield` | 1 | Blocks cooperative suspension | yes |
| `:debug` | 2 | Blocks breakpoints/tracing | yes |
| `:ffi` | 4 | Blocks foreign function calls | no |
| `:halt` | 8 | Blocks VM termination | yes |
| `:io` | 9 | Blocks operations that reach the I/O scheduler | yes |
| `:exec` | 11 | Blocks subprocess execution | no |
| `:gpu` | 15 | Blocks GPU dispatch | no |
| `:os-signal` | 16 | Blocks POSIX signal send/raise | no |
| `:fs` | 17 | Blocks filesystem access | no |

VM-internal signals (resume, propagate, abort, query, terminal,
switch, wait) cannot be denied.

## Filesystem authority is `:fs`, not `:io`

`:io` means "this request reaches the I/O scheduler". It is a dispatch
bit that a fiber can also deny, and it covers ports, sockets, and the
subprocess pipes — everything that suspends into the event loop.

The filesystem primitives are synchronous `std::fs` calls. They never
reach the scheduler, so they carry no `:io`, and denying `:io` never
stopped them. `:fs` is the bit that means "resolves a filesystem path",
and it is the one to deny to close the disk off:

```text
(fiber/new body |:error| :deny |:fs|)
```

The two bits are independent on purpose. A worker can keep `:io` — so it
still writes to stdout and talks to the network — while the disk is
denied and mediated by its parent. Deny both to close off all of it.

`port/open` carries both. It resolves a path (`:fs`) and it opens that
path through the scheduler (`:io`), so either denial blocks it. Without
the `:fs` bit on it, a fiber denied `:fs` alone could open a port on any
path and read it, which is the same authority `file/read` grants.

A primitive carries `:fs` when its implementation resolves a filesystem
path, decided by reading the implementation and not the name. `path/join`,
`path/parent`, and `path/normalize` are string operations and do not carry
it. `path/exists?`, `path/canonicalize`, and `path/cwd` reach the
filesystem and do.

## What happens on denial

When a fiber calls a primitive whose declared signal bits overlap
with the fiber's withheld capabilities, the primitive does not run.
Instead, the fiber emits a signal with:

- **Bits**: the blocked capability bits (e.g., `:io`)
- **Payload**: a struct describing the denial

```text
{:error :capability-denied
 :denied |:io|
 :primitive "port/read-line"
 :func <native-fn>
 :args ["arg1" "arg2"]}
```

The parent catches this signal through the normal mask routing.

## Introspection

```text
(fiber/caps)      # current fiber's capabilities
(fiber/caps f)    # specific fiber's capabilities
```

Returns a keyword set of active capabilities — everything in the
capability space that is NOT withheld:

```lisp
(fiber/caps)
# => |:debug :error :exec :ffi :fs :gpu :halt :io :os-signal :yield|

(let [f (fiber/new (fn [] 42) |:error| :deny |:fs :ffi|)]
  (fiber/caps f))
# => |:debug :error :exec :gpu :halt :io :os-signal :yield|
```

## Transitivity

Withheld capabilities propagate from parent to child at resume time.
A child inherits its parent's restrictions plus any `:deny` of its own:

```lisp
(let [outer (fiber/new
               (fn []
                 # inner denies :ffi, inherits :fs denial from outer
                 (let [inner (fiber/new (fn [] (fiber/caps))
                                        |:error| :deny |:ffi|)]
                   (fiber/resume inner)))
               |:error|
               :deny |:fs|)]
  (fiber/resume outer))
# => |:debug :error :exec :gpu :halt :io :os-signal :yield|
# (missing :fs from parent, missing :ffi from own deny)
```

A child can never gain capabilities its parent lacks. Requesting to
deny something the parent already withholds is a no-op (silently
absorbed).

Withheld capabilities also cross a thread. `sys/spawn` and
`sys/spawn-vm` run a deep-copied closure in a fresh VM, and that VM's
root fiber starts with the spawning fiber's withheld set. A worker
cannot reach what the fiber that spawned it could not.

A worker has no parent to suspend into, so a denial there cannot be
mediated. The thread ends instead, and the join reports it:

```text
(sys/join (sys/spawn-vm (fn [] (file/write path "x"))))
# from a fiber denying :fs => [:failed "..."], and nothing is written
```

Mediate on the fiber side of the boundary, before the work is handed to
a thread.

## Mediation

The parent can catch a denial, perform the operation on the child's
behalf, and resume the child with the result:

```lisp
(let [f (fiber/new
           (fn [] (length "hello"))
           |:error|
           :deny |:error|)]
  (let [denial (fiber/resume f)]
    # denial is {:error :capability-denied :primitive "length" ...}
    (let [val (fiber/value f)]
      (let [result (apply length (val :args))]
        (fiber/resume f result)))))
```

### Refusing

Resuming with an ordinary value tells the child the call succeeded and
returned that value. A mediator that wants to say *no* must not do that:
agent-written code reads the resume value as `file/write`'s return and
proceeds as though the write happened.

`fiber/refuse` raises the denied call as a failure **at the child's own
call site**. The child's `protect` or `try` catches it, and the child
keeps running:

```lisp
(with-temp-dir dir
  (let [target (path/join dir "notes")
        f (fiber/new
             (fn []
               (let [[ok? err] (protect (file/write target "x"))]
                 (list :refused (not ok?) err)))
             |:fs :error|
             :deny |:fs|)]
    (fiber/resume f)
    (fiber/refuse f :not-permitted)
    (fiber/value f)))
# => (:refused true :not-permitted)
```

The fiber stays alive. A refused call is an ordinary event in a mediated
session — the child is told no and carries on, and may be refused again
on its next call.

Refuse with `:fs`, not with `:error`. The child's own `protect` runs
primitives that declare `:error`, so a fiber denying that bit has no
working recovery path for the refusal to land in, and it ends `:error`
whatever the parent does.

An uncaught refusal is an ordinary uncaught error: it unwinds the child
through any `defer` blocks and the fiber ends `:error`. To end a fiber
outright rather than refuse one call, use `fiber/abort`.

## Specialized instructions

Arithmetic operations (`+`, `-`, `*`, `/`, comparisons) are compiled
to specialized bytecode instructions that bypass the primitive dispatch
path. These are not subject to capability checks. `:deny |:error|`
blocks `length` but not `+`.

## Examples

```text
# Pure computation sandbox — no IO, no filesystem, no FFI, no subprocess
(let [f (fiber/new compute |:io :fs :ffi :exec :error|
                    :deny |:io :fs :ffi :exec|)]
  (fiber/resume f))

# Mediated worker — keeps stdout and the network, mediates the disk
(let [f (fiber/new worker |:fs :error| :deny |:fs|)]
  (fiber/resume f))

# Capability-check a plugin before running it
(let [f (fiber/new plugin-init |:error| :deny |:exec :ffi|)]
  (let [result (fiber/resume f)]
    (if (= (fiber/status f) :dead)
      result
      (do (println "plugin tried:" ((fiber/value f) :primitive))
          (fiber/cancel f)))))

# Nested sandbox: outer denies IO, inner denies errors
(let [outer (fiber/new
               (fn []
                 (let [inner (fiber/new worker |:error| :deny |:error|)]
                   (fiber/resume inner)))
               |:io :error|
               :deny |:io|)]
  (fiber/resume outer))
# inner has neither IO (from outer) nor error (from own deny)
```
