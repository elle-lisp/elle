# Capability enforcement

<!-- audited: 2026-10-06 -->

Capabilities flow down. A fiber's parent decides what the fiber is
permitted to do. Operations the fiber can't perform become signals the
parent can catch.

## Creating a restricted fiber

`fiber/new` accepts `:deny` after the mask argument:

```lisp
(def body (fn [] :done))

(def no-io (fiber/new body |:io :error| :deny |:io|))            # deny IO
(def no-io-ffi (fiber/new body |:io :ffi :error| :deny |:io :ffi|))
(def open (fiber/new body |:error|))                             # no restriction

(assert (not (contains? (fiber/caps no-io) :io)))
(assert (contains? (fiber/caps open) :io))
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

The request is good only while the child waits on it. Service it before you
resume the child or let it go. A request raised after that raises
`:state-error` at the raise, and its operation never runs:

```lisp
(let [f (fiber/new (fn [] (port/write (port/stdout) "never written\n") :written) |:io|)
      req (fiber/resume f)]
  (fiber/resume f 0)                        # answer the write without running it
  (let [[ok? err] (protect (emit :io req))]
    (assert (not ok?))
    (assert (= (get err :error) :state-error))))
```

The refusal keeps a caught request from reading values its fiber already
released ([what a park retains](../impl/region/park.md)).

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
out to the scheduler. See [protocol.md](protocol.md).

| Keyword | Bit | Effect when denied | Dispatch |
|---------|-----|--------------------|----------|
| `:error` | 0 | Blocks every primitive that may error | yes |
| `:yield` | 1 | Blocks cooperative suspension | yes |
| `:debug` | 2 | Blocks breakpoints/tracing | yes |
| `:ffi` | 4 | Blocks foreign function calls | no |
| `:halt` | 8 | Blocks VM termination | yes |
| `:io` | 9 | Blocks operations that reach the I/O scheduler | yes |
| `:exec` | 11 | Blocks subprocess execution | no |
| `:gpu` | 15 | Blocks `git`, which compiles a closure to SPIR-V, and calls to a closure it compiled | no |
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

```lisp
(let [f (fiber/new (fn [] (file/read "/etc/hostname")) |:fs :error| :deny |:fs|)]
  (assert (= (get (fiber/resume f) :denied) |:fs|)))
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

- **Bits**: the blocked capability bits, for example `:io`
- **Payload**: a struct describing the denial

```lisp
(let [f (fiber/new (fn [] (length "hello")) |:error| :deny |:error|)
      denial (fiber/resume f)]
  (assert (= (get denial :error) :capability-denied))
  (assert (= (get denial :denied) |:error|))
  (assert (= (get denial :primitive) "length"))
  (assert (= (get denial :args) ["hello"])))   # :func holds the primitive itself
```

The parent catches this signal through the normal mask routing.

## A requirement can ride an argument, not only a name

A denial gates the primitive that **mints** authority, tested against the bits
that primitive declares. Some primitives instead spend authority a value carries,
and the value — not the primitive's name — says what the spend costs. For these
the gate reads the requirement from the argument and tests it against the calling
fiber, so a fiber cannot spend what it withholds however it obtained the value.

Two primitives work this way, each in its own domain:

- `io/submit` spends the operation its request argument carries. A spawn request
  needs `|:io :exec|`, an open needs `|:io :fs|`, a plain read needs `|:io|`.
- dynamic `emit` raises the bits its first argument names, so it needs those
  bits.

Loading a module needs no such rule, because each loader does one thing.
`import/load-plugin` runs a shared library's `elle_plugin_init`, which is
foreign code, so it declares `:ffi`. `import/load-file` loads Elle source and
declares `:fs` and no `:ffi`.

```lisp
# The first fiber may build the request; `f` may not spend it.
(let [req (fiber/resume
            (fiber/new (fn [] (subprocess/exec "/bin/sh" ["-c" "echo hi"]))
                       |:error :io :exec|))
      f (fiber/new (fn [r] (io/submit (io/backend :async) r))
                   |:error :io :exec| :deny |:exec|)]
  (fiber/resume f req)
  (assert (= (fiber/status f) :paused))
  (assert (= (get (fiber/value f) :primitive) "io/submit"))
  (assert (= (get (fiber/value f) :denied) |:exec|)))
```

Three edges the gate does not reach. `io/submit` tests the fiber that submits, so
a scheduler that submits a request on another fiber's behalf spends its own
authority. A loaded module's own primitives carry whatever bits the plugin
declared, so a fiber handed a module still reaches them. And the literal `emit`
compiles to a bytecode instruction rather than a call, so a fiber raises any
literal bit it names — a mask therefore audits a cooperative child, not a hostile
one. [authority.md](authority.md) states the model these follow from, and
[emit.md](emit.md) covers what `emit` does not check.

## Introspection

`(fiber/caps)` answers for the current fiber, and `(fiber/caps f)` for the
fiber `f`. Each returns a keyword set of active capabilities — everything in the
capability space that is NOT withheld:

```lisp
(fiber/caps)
# => |:debug :error :exec :ffi :fs :gpu :halt :io :os-signal :yield|

(let [f (fiber/new (fn [] 42) |:error| :deny |:fs :ffi|)]
  (fiber/caps f))
# => |:debug :error :exec :gpu :halt :io :os-signal :yield|
```

## Transitivity

A fiber takes the withheld set of the fiber that creates it, when `fiber/new`
makes it. Each resume then adds the withheld set of the fiber that resumes it.
A child inherits both restrictions plus any `:deny` of its own:

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

A child can never gain a capability that its creator lacks, or that any
fiber which resumes it lacks. Requesting to deny something the parent
already withholds is a no-op (silently absorbed).

The creator's half is what holds when a fiber leaves the sandbox that made
it. A fiber that code inside the sandbox creates keeps the denial, even
when a fiber outside the sandbox resumes it:

```lisp
(let [sandbox (fiber/new (fn [] (fiber/new (fn [] (fiber/caps)) |:error|))
                         |:error| :deny |:fs|)
      made (fiber/resume sandbox)]
  (assert (not (contains? (fiber/caps made) :fs)) "the new fiber lacks :fs")
  (assert (not (contains? (fiber/resume made) :fs))
          "and still lacks it when an unrestricted fiber resumes it"))
```

Withheld capabilities also cross a thread. `sys/spawn` and
`sys/spawn-vm` run a deep-copied closure in a fresh VM, and that VM's
root fiber starts with the spawning fiber's withheld set. A worker
cannot reach what the fiber that spawned it could not.

A worker has no parent to suspend into, so a denial there cannot be
mediated. The thread ends instead, and the join raises `:thread-error`:

```lisp
(with-temp-dir dir
  (let [target (path/join dir "x")
        f (fiber/new (fn [] (sys/join (sys/spawn-vm (fn [] (file/write target "x")))))
                     |:error| :deny |:fs|)]
    (assert (= (get (fiber/resume f) :error) :thread-error))
    (assert (not (path/exists? target)))))   # nothing is written
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
working recovery path for the refusal to land in.

An uncaught refusal stops the child `:error` at the refused call. With
`:error` in the child's mask, `fiber/refuse` answers the refusal and
`fiber/value` holds it. Without `:error`, the refusal raises past
`fiber/refuse` into the mediator, as an error passes `fiber/resume`:

```lisp
(let [f (fiber/new (fn [] (file/read "/etc/hostname")) |:fs| :deny |:fs|)]
  (fiber/resume f)
  (let [[ok? err] (protect (fiber/refuse f :not-permitted))]
    (assert (not ok?))
    (assert (= err :not-permitted))
    (assert (= (fiber/status f) :error))))
```

The mediator then decides what the stopped child does. `fiber/resume`
restarts it at the refused call, and the resume value answers the call
exactly as a grant would. `fiber/cancel` ends it `:dead`, and the code past
the call never runs:

```lisp
(with-temp-dir dir
  (let [target (path/join dir "notes")
        f (fiber/new (fn [] (list :wrote (file/write target "x")))
                     |:fs :error| :deny |:fs|)]
    (fiber/resume f)
    (assert (= (fiber/refuse f :not-permitted) :not-permitted))
    (assert (= (fiber/status f) :error))
    (assert (= (fiber/value f) :not-permitted))
    (assert (= (fiber/resume f 1) (list :wrote 1)))   # the restart answers the call
    (assert (not (path/exists? target)))))            # and nothing was written

(let [ran @[]
      f (fiber/new (fn [] (file/read "/etc/hostname") (push ran :continued))
                   |:fs :error| :deny |:fs|)]
  (fiber/resume f)
  (fiber/refuse f :not-permitted)
  (fiber/cancel f)
  (assert (= (fiber/status f) :dead))
  (assert (empty? ran)))                              # the continuation never ran
```

To end a fiber outright rather than refuse one call, use `fiber/cancel`.

## A fiber a scheduler runs

`ev/spawn` creates a fiber in the calling fiber and hands it to the scheduler,
which resumes it. The spawned fiber therefore carries its creator's denial. Its
resumer is the scheduler, though, and the scheduler is not the mediator that
imposed the denial.

So the scheduler refuses the call. It hands the fiber `fiber/refuse`, with the
denial payload as the error, and the fiber's `protect` sees that payload at its
own call site. An uncaught refusal ends the fiber, and `ev/join` raises the
payload in the fiber that joins it:

```lisp
(with-temp-dir dir
  (let [target (path/join dir "x")
        sandbox (fiber/new
                  (fn []
                    (let [caught (ev/join (ev/spawn (fn [] (protect (file/write target "x")))))
                          raised (protect (ev/join (ev/spawn (fn [] (file/write target "x")))))]
                      [caught raised]))
                  |:fs :error| :deny |:fs|)
        [[ok1? denial] [ok2? raised]] (fiber/resume sandbox)]
    (assert (not ok1?) "the spawned fiber's protect sees the refusal")
    (assert (= (get denial :error) :capability-denied) "carrying the denial")
    (assert (= (get denial :primitive) "file/write") "which names the call")
    (assert (not ok2?) "an uncaught refusal ends the fiber, and the join raises it")
    (assert (= (get raised :error) :capability-denied))
    (assert (not (path/exists? target)) "nothing is written")))
```

A spawned fiber's mask names the bits a denial raises, `:ffi`, `:exec`, `:gpu`,
`:os-signal` and `:fs`, as well as the bits the scheduler's own protocol uses.
A mediator above the scheduler therefore never sees a spawned fiber's denial.
The process scheduler does the same for a process and for each fiber a process
spawns, and `process:spawn` creates the new process's fiber in the fiber that
calls it ([process-scheduler.md](../process-scheduler.md)).

## A macro expands with the fiber's capabilities

Code on a fiber can start a compile: `eval`, a module loader, `compile/analyze`
and the other `compile/*` primitives that compile source. Every macro that
compile expands runs with the capabilities the fiber withholds, so a fiber
cannot spend at expansion time what its own code cannot spend. An expansion
cannot pause, so a parent cannot mediate a denial there. The compile fails,
and the error names the macro and the denied primitive:

```lisp
(let [f (fiber/new (fn [] (eval '(begin (defmacro m [] (file/read "/etc/hostname")) (m))))
                   |:fs :error| :deny |:fs|)
      err (fiber/resume f)]
  (assert (= (get err :error) :eval-error) "the compile fails")
  (assert (string/contains? (get err :message) "macro 'm'") "naming the macro")
  (assert (string/contains? (get err :message) "file/read") "and the primitive"))
```

An `include` or `include-file` reads its file for that compile, so a fiber
that withholds `:fs` cannot include a file either. The file `elle` runs
compiles before any fiber exists, so its macros run with every capability.

## Intrinsics are not checked

The `%` [intrinsics](../intrinsics.md) compile to bytecode instructions that
bypass primitive dispatch, so no capability check reaches them. The generic
`+` is a stdlib function, and a denial reaches it through a primitive it
calls — the internal-helper seam #930 describes:

```lisp
(def a 1)
(def b 2)
(let [f (fiber/new (fn [] (%add a b)) |:error| :deny |:error|)]
  (assert (= (fiber/resume f) 3)))                      # %add runs
(let [f (fiber/new (fn [] (+ a b)) |:error| :deny |:error|)]
  (assert (= (get (fiber/resume f) :primitive) "empty?")))  # + is denied inside
```

## Examples

Each sandbox below holds against the primitives the fiber names. A
capability-bearing value handed in from outside is confined only where a crossing
checks it: a submitted request is; a module already loaded and handed in is
not — see the spend section above.

```lisp
(defn compute [] (* 6 7))
(defn worker [] (file/read "/etc/hostname"))
(defn plugin-init [] (subprocess/system "true" []))

# Pure computation sandbox — no IO, no filesystem, no FFI, no subprocess
(let [f (fiber/new compute |:io :fs :ffi :exec :error|
                    :deny |:io :fs :ffi :exec|)]
  (assert (= (fiber/resume f) 42)))

# Mediated worker — keeps stdout and the network; the parent sees each disk access
(let [f (fiber/new worker |:fs :error| :deny |:fs|)]
  (assert (= (get (fiber/resume f) :primitive) "file/read")))

# Watch what a plugin's init tries to do
(let [f (fiber/new plugin-init |:error :exec :ffi| :deny |:exec :ffi|)]
  (fiber/resume f)
  (assert (not (= (fiber/status f) :dead)))
  (assert (= (get (fiber/value f) :primitive) "subprocess/exec"))
  (fiber/cancel f))
```

Denying `:ffi` stops the fiber loading a native module, because
`import/load-plugin` declares `:ffi` for the foreign code its load runs. The
gate refuses the call before the loader reads the path, so no library need
exist. A `.lisp` module loads through `import/load-file`, which needs only
`:fs`.

```lisp
(let [f (fiber/new (fn [] (import/load-plugin "nonexistent.so")) |:ffi :error|
                   :deny |:ffi|)]
  (assert (= (get (fiber/resume f) :primitive) "import/load-plugin")))
```

```lisp
# Nested sandbox: outer denies IO, inner denies errors
(let [outer (fiber/new
               (fn []
                 (let [inner (fiber/new (fn [] :done) |:error| :deny |:error|)]
                   (fiber/resume inner)
                   (fiber/caps inner)))   # read from outside: fiber/caps itself declares :error
               |:io :error|
               :deny |:io|)
      caps (fiber/resume outer)]
  (assert (not (contains? caps :io)))      # from outer
  (assert (not (contains? caps :error))))  # from its own deny
```
