# Capability enforcement

<!-- audited: 2026-10-11 -->

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
against the bits a primitive *declares* and the bits an `emit` names, so
any such bit is one a parent can withhold. Some bits also drive dispatch —
`:io` routes a request through the scheduler — and some do no dispatch work
at all. That distinction belongs to the VM. It changes neither what a mask
can deny nor what it can catch.

`:yield` is the cooperative suspension `(yield v)` raises, and no I/O
request carries it: a request raises `|:io|` alone. So a generator masked
`|:yield|` catches its own yields while its reads travel out to the
scheduler, and a fiber denied `:yield` cannot yield but can still read. See
[protocol.md](protocol.md).

| Keyword | Bit | Effect when denied | Dispatch |
|---------|-----|--------------------|----------|
| `:error` | 0 | Blocks every primitive that may error, and `error` | yes |
| `:yield` | 1 | Blocks cooperative suspension, `yield` included | yes |
| `:debug` | 2 | Blocks breakpoints/tracing | yes |
| `:ffi` | 4 | Blocks foreign function calls | no |
| `:halt` | 8 | Blocks VM termination | yes |
| `:io` | 9 | Blocks operations that reach the I/O scheduler | yes |
| `:exec` | 11 | Blocks subprocess execution | no |
| `:gpu` | 15 | Blocks `git`, which compiles a closure to SPIR-V, and calls to a closure it compiled | no |
| `:os-signal` | 16 | Blocks POSIX signal send/raise | no |
| `:fs` | 17 | Blocks filesystem access | no |
| `:net` | 18 | Blocks sockets and name resolution | no |

VM-internal signals (resume, propagate, query, terminal, fuel, switch,
wait) cannot be denied.

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

## Network authority is `:net`, not `:io`

`:net` is the bit that means "opens a socket or asks a name server". Every
socket primitive carries it: the `tcp/`, `udp/` and `unix/` primitives, and
`sys/resolve`. `sys/ip?` only reads a string, so it carries nothing.

Denying `:io` closes the network too, because a primitive that waits on a peer
reaches the scheduler. It also closes stdout, pipes and `ev/sleep`. Deny `:net`
to close the network alone:

```lisp
(let [f (fiber/new (fn [] (tcp/listen "127.0.0.1" 0)) |:net :error| :deny |:net|)]
  (assert (= (get (fiber/resume f) :denied) |:net|)))

(let [f (fiber/new (fn [] (ev/sleep 0) (port/flush (port/stdout)) :done)
                   |:error| :deny |:net|)]
  (assert (= (fiber/resume f) :done) "timers and stdout stay open"))
```

A socket primitive carries every bit its implementation needs, so any one
denial blocks it. One that waits on a peer also carries `:io`. `unix/listen`
and `unix/connect` resolve a filesystem path, so they also carry `:fs`, and
`unix/connect` carries `|:fs :net :io|`.

A port a fiber already holds needs no `:net`. `port/read` and `port/write`
carry `:io` alone on a socket, as they carry no `:fs` on a file. The denial
gates the primitive that opens a connection, so a parent that hands a child a
connected port grants it that connection.

## What happens on denial

When a fiber calls a primitive whose declared signal bits overlap
with the fiber's withheld capabilities, the primitive does not run.
An `emit` of a withheld bit raises nothing, as a call to `fiber/emit`
would not run ([emit.md](emit.md)). Instead, the fiber emits a signal with:

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
  needs `|:io :exec|` and an open needs `|:io :fs|`. A socket operation needs
  `|:io :net|`: a connect, an accept, a datagram, a shutdown or a name lookup.
  A connect to a Unix socket adds `:fs`, and a plain read needs `|:io|`.
- `emit` raises the bits its first argument names, so it needs those bits. The
  literal form compiles to the `Emit` instruction rather than a call, and that
  instruction asks the same question of the bits it carries
  ([emit.md](emit.md)).

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

Two edges the gate does not reach. `io/submit` tests the fiber that submits, so
a scheduler that submits a request on another fiber's behalf spends its own
authority. A loaded module's own primitives carry whatever bits the plugin
declared, so a fiber handed a module still reaches them.
[authority.md](authority.md) states the model these follow from.

Since no fiber raises a bit it withholds, a parent that denies a bit and names
it in the mask sees every attempt to raise it, from a hostile child as from a
cooperative one.

## Introspection

`(fiber/caps)` answers for the current fiber, and `(fiber/caps f)` for the
fiber `f`. Each returns a keyword set of active capabilities — everything in the
capability space that is NOT withheld:

```lisp
(fiber/caps)
# => |:debug :error :exec :ffi :fs :gpu :halt :io :net :os-signal :yield|

(let [f (fiber/new (fn [] 42) |:error| :deny |:fs :ffi|)]
  (fiber/caps f))
# => |:debug :error :exec :gpu :halt :io :net :os-signal :yield|
```

## Reading a body against its mask and deny set

A `fiber/new` call can show both directions at once. When its body is a
function literal and its mask and `:deny` set are literal keywords or sets,
the compiler knows the signals the body may raise, the signals its parent
catches, and the capabilities its parent withholds. The linter then gives two
hints:

- **A call that will be denied**: the body may raise a bit the `:deny` set
  withholds, so the call that raises it does not run, and its denial reaches
  the parent instead.
- **A signal that will pass the parent**: the body may raise a bit the mask
  does not name, so a signal that carries only that bit passes the parent to
  the fiber above it.

Each is a hint, not a warning. A denial is how the language mediates a call,
and a signal passing the parent is how a request reaches the scheduler. The
hints say what the program does, so that a reader sees it without tracing the
bits by hand. A body that is not a function literal, and a mask or `:deny` set
computed at run time, give no hint, because the compiler does not know one of
the sides. [lint-rules.md](../cookbook/lint-rules.md) lists the codes.

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
# => |:debug :error :exec :gpu :halt :io :net :os-signal :yield|
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

A parent that catches a denial decides what the denied call does: it can
perform the call and resume the child with the result, or refuse it with
`fiber/refuse`. A fiber a scheduler runs is refused by that scheduler.
[mediation.md](mediation.md) holds all three.
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
