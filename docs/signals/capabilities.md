# Capability enforcement

<!-- audited: 2026-11-21 -->

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
the operations that raise it. `:exec` catches subprocess requests, `:fs`
catches the filesystem denials, and `:io` catches scheduler requests. There
is no bit a mask must name before another bit takes effect.

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
at all. That distinction changes neither what a mask can deny nor what it
can catch.

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
| `:ambient` | 19 | Blocks reads and changes of process state that no other bit names | no |
| `:import` | 20 | Blocks module loads: `import` emits it, and a handler answers it | no |
| `:inspect` | 21 | Blocks reads of the state a value encloses: `fn/captures`, `fiber/child` | no |

VM-internal signals (resume, propagate, query, terminal, fuel, switch,
wait) cannot be denied. [bits.md](bits.md) says what `:fs`, `:net`,
`:ambient` and `:inspect` each gate, and which primitives carry each one.

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
  needs `|:io :exec|`, and an open or a file operation by path needs
  `|:io :fs|`. A socket operation needs
  `|:io :net|`: a connect, an accept, a datagram, a shutdown or a name lookup.
  A connect to a Unix socket adds `:fs`, and a plain read needs `|:io|`.
- `emit` raises the bits its first argument names, so it needs those bits. The
  literal form compiles to the `Emit` instruction rather than a call, and that
  instruction asks the same question of the bits it carries
  ([emit.md](emit.md)).

Loading a module needs no such rule, because each loader does one thing.
`import/load-plugin` runs a shared library's `elle_plugin_init`, which is
foreign code, so it declares `:ffi`. `import/load-file` is Elle code that
reads a file, so a fiber denied `:fs` cannot call it, and `import` emits
`:import`, which the handler above the fiber answers ([modules](../modules.md)).

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
fiber `f`. Each returns the keyword set of every capability bit the fiber does
not withhold:

```lisp
(let [f (fiber/new (fn [] 42) |:error| :deny |:fs :ffi|)]
  (assert (not (contains? (fiber/caps f) :fs)))
  (assert (contains? (fiber/caps f) :import)))
```

## Reading a body against its mask and deny set

A `fiber/new` call can show both directions at once. Its body may be a
function literal, and its mask and `:deny` set literal keywords or sets. Then
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
and a signal passing the parent is how a request reaches the scheduler. A body
that is not a function literal, and a mask or `:deny` set computed at run time,
give no hint, because the compiler does not know one of the sides.
[lint-rules.md](../cookbook/lint-rules.md) lists the codes.

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
# => |:ambient :debug :error :exec :gpu :halt :import :inspect :io :net
#     :os-signal :yield|
# (missing :fs from parent, missing :ffi from own deny)
```

A child can never gain a capability that its creator lacks, or that any
fiber which resumes it lacks. Denying what the parent withholds changes
nothing.

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
mediated. The thread ends instead, and the join raises `:thread-error`.
`path/cwd` does not reach the scheduler, so only the denial stops it. A call
that reaches the scheduler ends a worker with no `ev/run` whatever it denies
([threads](../threads.md)):

```lisp
(assert (string? (sys/join (sys/spawn-vm (fn [] (path/cwd)))))
        "a worker that withholds nothing reads the working directory")
(let [f (fiber/new (fn [] (sys/join (sys/spawn-vm (fn [] (path/cwd)))))
                   |:error| :deny |:fs|)]
  (assert (= (get (fiber/resume f) :error) :thread-error)
          "a worker that withholds :fs ends instead"))
```

Mediate on the fiber side of the boundary, before the work is handed to
a thread.

## Mediation

A parent that catches a denial decides what the denied call does: it can
perform the call and resume the child with the result, or refuse it with
`fiber/refuse`. A fiber a scheduler runs is refused by that scheduler.
[mediation.md](mediation.md) holds all three. A denial inside a macro
expansion reaches no parent, and the compile fails ([macros.md](../macros.md)).

## Intrinsics are not checked

The `%` [intrinsics](../intrinsics.md) compile to bytecode instructions that
bypass primitive dispatch, so no capability check reaches them. The generic
`+` is a stdlib function, and a call to it becomes its body
([grafting a call](../impl/graft.md)). Over two proven numbers that body is
one `%add`, so it runs under any denial. Over an operand nothing proves, the
body keeps its number guard, and a denial reaches the primitive the guard
calls. That is the internal-helper seam #930 describes:

```lisp
(def a 1)
(def b 2)
(let [f (fiber/new (fn [] (%add a b)) |:error| :deny |:error|)]
  (assert (= (fiber/resume f) 3)))                      # %add runs
(let [f (fiber/new (fn [] (+ a b)) |:error| :deny |:error|)]
  (assert (= (fiber/resume f) 3)))                      # so does + over two ints

(defn add [x y] (+ x y))
(def add-value add)                    # a value use proves nothing about x or y
(let [f (fiber/new (fn [] (add-value a b)) |:error| :deny |:error|)]
  (assert (= (get (fiber/resume f) :primitive) "type-of")))  # the guard is denied
```

## Examples

Each sandbox below holds against the primitives the fiber names. A
capability-bearing value handed in from outside is confined only where a
crossing checks it: a submitted request is, and a loaded module is not.

```lisp
(defn compute [] (* 6 7))
(defn plugin-init [] (subprocess/system "true" []))

# Pure computation sandbox — no IO, filesystem, FFI, subprocess, ambient state
# or enclosed state
(let [f (fiber/new compute |:io :fs :ffi :exec :ambient :inspect :error|
                    :deny |:io :fs :ffi :exec :ambient :inspect|)]
  (assert (= (fiber/resume f) 42)))

# Watch what a plugin's init tries to do
(let [f (fiber/new plugin-init |:error :exec :ffi| :deny |:exec :ffi|)]
  (fiber/resume f)
  (assert (not (= (fiber/status f) :dead)))
  (assert (= (get (fiber/value f) :primitive) "subprocess/exec"))
  (fiber/cancel f))
```

Denying `:ffi` stops a native module load. The gate refuses the call before
the loader reads the path, so no library need exist:

```lisp
(let [f (fiber/new (fn [] (import/load-plugin "nonexistent.so")) |:ffi :error|
                   :deny |:ffi|)]
  (assert (= (get (fiber/resume f) :primitive) "import/load-plugin")))
```
