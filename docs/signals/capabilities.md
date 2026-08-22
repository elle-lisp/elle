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

## Deniable capabilities

Every signal bit is a capability bit. A fiber's `:deny` set is tested
against the bits a primitive *declares*, so any bit a primitive declares
is one a parent can withhold. Some bits also drive dispatch — `:io`
routes a request through the scheduler, `:yield` suspends — and some do
no dispatch work at all. That distinction belongs to the VM. It does not
change what a mask can deny.

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
