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

The following signal keywords can be denied:

| Keyword | Bit | Effect when denied |
|---------|-----|--------------------|
| `:error` | 0 | Blocks primitives that may error (~66% of all) |
| `:yield` | 1 | Blocks cooperative suspension |
| `:debug` | 2 | Blocks breakpoints/tracing |
| `:ffi` | 4 | Blocks foreign function calls |
| `:halt` | 8 | Blocks VM termination |
| `:io` | 9 | Blocks I/O operations |
| `:exec` | 11 | Blocks subprocess execution |

VM-internal signals (resume, propagate, abort, query, terminal,
switch, wait) cannot be denied.

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

```text
(fiber/caps)
# => |:error :yield :debug :ffi :halt :io :exec|

(let ([f (fiber/new (fn [] 42) |:error| :deny |:io :ffi|)])
  (fiber/caps f))
# => |:error :yield :debug :halt :exec|
```

## Transitivity

Withheld capabilities propagate from parent to child at resume time.
A child inherits its parent's restrictions plus any `:deny` of its own:

```text
(let ([outer (fiber/new
               (fn []
                 # inner denies :ffi, inherits :io denial from outer
                 (let ([inner (fiber/new (fn [] (fiber/caps))
                                        |:error| :deny |:ffi|)])
                   (fiber/resume inner)))
               |:error|
               :deny |:io|)])
  (fiber/resume outer))
# => |:error :yield :debug :halt :exec|
# (missing :io from parent, missing :ffi from own deny)
```

A child can never gain capabilities its parent lacks. Requesting to
deny something the parent already withholds is a no-op (silently
absorbed).

## Mediation

The parent can catch a denial, perform the operation on the child's
behalf, and resume the child with the result:

```text
(let ([f (fiber/new
           (fn [] (length "hello"))
           |:error|
           :deny |:error|)])
  (let ([denial (fiber/resume f)])
    # denial is {:error :capability-denied :primitive "length" ...}
    (let ([val (fiber/value f)])
      (let ([result (apply length (val :args))])
        (fiber/resume f result)))))
```

## Specialized instructions

Arithmetic operations (`+`, `-`, `*`, `/`, comparisons) are compiled
to specialized bytecode instructions that bypass the primitive dispatch
path. These are not subject to capability checks. `:deny |:error|`
blocks `length` but not `+`.

## Examples

```text
# Pure computation sandbox — no IO, no FFI, no subprocess
(let ([f (fiber/new compute |:io :ffi :exec :error|
                    :deny |:io :ffi :exec|)])
  (fiber/resume f))

# Capability-check a plugin before running it
(let ([f (fiber/new plugin-init |:error| :deny |:exec :ffi|)])
  (let ([result (fiber/resume f)])
    (if (= (fiber/status f) :dead)
      result
      (do (println "plugin tried:" ((fiber/value f) :primitive))
          (fiber/cancel f)))))

# Nested sandbox: outer denies IO, inner denies errors
(let ([outer (fiber/new
               (fn []
                 (let ([inner (fiber/new worker |:error| :deny |:error|)])
                   (fiber/resume inner)))
               |:io :error|
               :deny |:io|)])
  (fiber/resume outer))
# inner has neither IO (from outer) nor error (from own deny)
```
