# Capability bits

<!-- audited: 2026-11-21 -->

What `:fs`, `:net`, `:ambient` and `:inspect` each gate, and how to tell which of them a primitive carries.

A primitive declares the bits its implementation needs, and a fiber that
withholds one of them cannot call it. [capabilities.md](capabilities.md) owns
the withheld set, the list of bits and the denial.

## Filesystem authority is `:fs`, not `:io`

`:io` means "this request reaches the I/O scheduler". It is a dispatch
bit that a fiber can also deny, and it covers ports, sockets, the
subprocess pipes and the file calls — everything that suspends into the
event loop.

`:fs` is the bit that means "resolves a filesystem path", and it is the
one to deny to close the disk off:

```lisp
(let [f (fiber/new (fn [] (file/read "/etc/hostname")) |:fs :error| :deny |:fs|)]
  (assert (= (get (fiber/resume f) :denied) |:fs|)))
```

The two bits are independent. A worker can keep `:io`, and still write to
stdout and talk to the network, while its parent denies and mediates the disk.

A call that reaches the disk through the scheduler carries both, so either
denial blocks it. `port/open` resolves a path (`:fs`) and opens it through
the scheduler (`:io`). Without the `:fs` bit on it, a fiber denied `:fs`
alone could open a port on any path and read it, which is the same authority
`file/read` grants. The `file/` primitives that take a path, and the probes
`path/exists?`, `path/file?`, `path/dir?` and `path/canonicalize`, carry both
for the same reason. `file/read` and the other calls that move a file's
contents open a port, so their denial lands at the open
([files](../io/files.md)).

`path/cwd`, `path/absolute`, `import/load-plugin`, `watch-add`, `watch-remove`
and `unix/listen` resolve a path without the scheduler, so they carry `:fs` and
no `:io`, and denying `:io` does not stop them:

```lisp
(let [f (fiber/new (fn [] (path/cwd)) |:io :error| :deny |:io|)]
  (assert (string? (fiber/resume f)) ":deny |:io| leaves path/cwd open"))
```

A primitive carries `:fs` when its implementation resolves a filesystem
path, decided by reading the implementation and not the name. `path/join`,
`path/parent`, and `path/normalize` are string operations and do not carry
it.

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
denial blocks it. One that waits on a peer also carries `:io`, and a Unix
socket's path adds `:fs`.

A port a fiber already holds needs no `:net`. `port/read` and `port/write`
carry `:io` alone on a socket, as they carry no `:fs` on a file. The denial
gates the primitive that opens a connection, so a parent that hands a child a
connected port grants it that connection.

## Ambient state is `:ambient`

`:ambient` is the bit that means "reads or changes process state that no
argument carries and no other bit names". The environment, the arguments and
the pid carry it, and so do the clocks, `vm/config`, the VM's gauges, the
signal registry, `hash` and the standard ports. A fiber denied it computes from
the values it was handed and nothing else:

```lisp
(let [f (fiber/new (fn [] (sys/env "HOME")) |:ambient :error| :deny |:ambient|)]
  (assert (= (get (fiber/resume f) :denied) |:ambient|)))

(let [f (fiber/new (fn [] (string/join ["a" "b"] "-")) |:error| :deny |:ambient|)]
  (assert (= (fiber/resume f) "a-b") "computation over its values runs"))

(assert (contains? (get (vm/primitive-meta "sys/env") :bits) :ambient)
        "a primitive's metadata names the bits it declares")
```

Every macro expansion withholds `:ambient`, so a macro body cannot read the
clock or the environment ([macros.md](../macros.md)). A function that calls an
ambient primitive carries `:ambient` in its inferred signal, so it is not
silent.

## Enclosed state is `:inspect`

`:inspect` is the bit that means "reads the state a value encloses". A closure
gives its holder a call and nothing more, and the bindings it captured stay
private to the code that wrote it ([modules](../modules.md)). `fn/captures`
answers each captured value, so it carries `:inspect`:

```lisp
(def counter (let [items @[1 2 3]] (fn [] (length items))))

(let [f (fiber/new (fn [] (fn/captures counter)) |:inspect :error| :deny |:inspect|)
      denial (fiber/resume f)]
  (assert (= (get denial :denied) |:inspect|))
  (assert (= (get denial :primitive) "fn/captures")))

(let [f (fiber/new (fn [] (counter)) |:error| :deny |:inspect|)]
  (assert (= (fiber/resume f) 3) "a call of the closure runs"))

(push (get (first (fn/captures counter)) :value) 4)
(assert (= (counter) 4) "a fiber that holds :inspect reads the array, and can change it")
```

A fiber encloses state too. `fiber/child` answers the child whose signal last
passed through a fiber, which no argument names, so it carries `:inspect`:

```lisp
(def holder (fiber/new (fn [] (fiber/resume (fiber/new (fn [] (yield 1)) ||)))
                       |:yield|))
(fiber/resume holder)

(let [f (fiber/new (fn [] (fiber/child holder)) |:inspect :error| :deny |:inspect|)]
  (assert (= (get (fiber/resume f) :primitive) "fiber/child")))

(assert (fiber? (fiber/child holder)) "a fiber that holds :inspect reads the child")
```

A captured value may be a mutable collection, and a fiber that reads the
collection can change it. A scheduler keeps its queues in such captures, and a
fiber reaches the scheduler's functions through a parameter. So a sandbox that
withholds the effect capabilities withholds `:inspect` beside them. Every macro
expansion does ([macros.md](../macros.md)), and so does every module's form.

`fn/arity`, `fn/errors?` and the other closure predicates read the closure's
code and none of its captures, so they carry no `:inspect`
([debugging](../analysis/debugging.md)).
