# gun

<!-- audited: 2026-10-12 -->

A graph database in the shape of GUN: nodes merged field by field by HAM, on a memory or Redis store, joined to peers by WebSocket.

The export struct at the bottom of [gun.lisp](gun.lisp) lists what the module
offers, and `(doc name)` carries each function's arguments.
[gun/overview.md](gun/overview.md) lists the submodules behind it. This file
holds the data model, the merge rule, the store layout and the invariants.

The design follows [GUN](https://gun.eco/) and its HAM conflict rule. It does
not speak GUN's wire format, and it has no SEA yet. Databases join over
WebSocket; [gun/mesh.md](gun/mesh.md) holds the peer protocol.

## Loading

Called with no argument, the module has the memory store only and no mesh.
`gun:redis-store` then raises
`{:error :gun-error :reason :redis-not-configured}`, and `gun:serve` and
`gun:connect` raise `:mesh-not-configured`. The Redis store needs the
`std/redis` module as `:redis`. The mesh needs the `std/websocket` module as
`:websocket`, built from the plugins that `(import "plugin/hash")` and
`(import "plugin/random")` load:

```lisp
(def gun ((import "std/gun")))

(defn gun-on-redis []
  "The gun module, able to build a Redis store."
  ((import "std/gun") :redis ((import "std/redis"))))

(defn gun-with-mesh [hash-plugin random-plugin]
  "The gun module, able to join peers."
  ((import "std/gun")
   :websocket ((import "std/websocket") :hash hash-plugin
                                        :random random-plugin)))

(let [[ok? err] (protect (gun:connect (gun:open) "ws://127.0.0.1:1/gun"))]
  (assert (= (get err :reason) :mesh-not-configured)))
```

## The data model

A database is a graph of **nodes**. A **soul** names each node, and a node
holds **fields**. Each field has a string key, a value and a **state**.

A value is one of these:

- `nil`, which is a **tombstone**: the field is deleted, and its state still
  takes part in the merge.
- a boolean, an integer, a finite float, an immutable string or immutable
  bytes.
- a **link** to another node, built by `gun:link` from a soul.

Nothing else is a value: not an array, a list, a set, a keyword, a closure, a
NaN, an infinity, or a mutable string or bytes. A `put` that holds one raises
`{:error :gun-error :reason :invalid-value}`, with the key path in `:path`, and
writes nothing.

A key is a string. A keyword key enters by its name, so `:age` and `"age"` are
the same key, and a read returns string keys. The key `_` is reserved, and a
`put` that uses it, or a key of any other type, raises
`{:error :gun-error :reason :invalid-key}`.

## Chains

A database is a chain, and `gun:get` extends a chain by one key. The first key
names a root node. Each later key names a field of the node before it. The
database itself names no node, so a `put` or a `once` on it raises
`{:error :gun-error :reason :no-soul}`. A root holds a node, so a `put` of
anything but a struct on a root raises `:not-a-node`:

```lisp
(def db (gun:open))
(def alice (gun:get (gun:get db "users") "alice"))

(gun:put alice {:name "Alice" :age 30})
(assert (= (gun:once alice) {"name" "Alice" "age" 30}))
(assert (= (gun:once (gun:get alice "name")) "Alice"))
```

A nested struct becomes a node of its own, and the parent field holds a link to
it. When the field already holds a link, the struct is written to the node the
link names. Otherwise the child's soul is the parent's soul, a `/`, and the
key, so two writers who create the same nested node create one node, not two:

```lisp
(def users (gun:once (gun:get db "users")))
(assert (gun:link? (get users "alice")))
(assert (= (gun:link-soul (get users "alice")) "users/alice"))
```

A soul escapes `%` as `%25` and `/` as `%2F` in each of its parts. The key
`a/b` of the root `x` is therefore the soul `x/a%2Fb`, and the key `b` of the
key `a` of `x` is `x/a/b`.

`gun:once` answers a chain with its value. A chain that reaches a node, at a
root or through a link, answers a struct of the node's live fields, and a link
inside that struct stays a link. Any other chain answers the value of its
field. A chain that names nothing answers `nil`, and a tombstone leaves its key
out of the struct:

```lisp
(assert (nil? (gun:once (gun:get db "nobody"))))
(gun:put (gun:get alice "age") nil)
(assert (= (gun:once alice) {"name" "Alice"}))
```

`gun:on` calls its function at once with the chain's value, and again whenever
a write changes that value, whether the write is local or comes from a peer. It
returns a function that ends the subscription. A write that leaves the value as
it was calls nothing:

```lisp
(def seen @[])
(def stop (gun:on (gun:get alice "name") (fn [v] (push seen v))))
(gun:put (gun:get alice "name") "Alicia")
(gun:put (gun:get alice "name") "Alicia")
(stop)
(gun:put (gun:get alice "name") "Ali")
(assert (= (freeze seen) ["Alice" "Alicia"]))
```

`gun:set` adds a node to an unordered collection. It gives the node a random
soul, links it from the collection under that soul, and returns the chain of
the new node:

```lisp
(def todo (gun:get db "todo"))
(def item (gun:set todo {:title "write docs"}))
(assert (= (gun:once (gun:get item "title")) "write docs"))
(assert (= (length (gun:once todo)) 1))
```

## The merge rule

Each write carries a state, and HAM compares it with the state of the field it
would replace. `gun:ham` takes the merge's current state and the two
state-value pairs:

| Verdict | When | Effect |
|---------|------|--------|
| `:future` | the incoming state is after now | not applied |
| `:stale` | the incoming state is before the current state | dropped |
| `:same` | equal states, and the incoming value does not sort after the current one | dropped |
| `:accept` | otherwise | applied |

A field that does not exist yet has no state, so its first write is accepted.
On equal states the value whose encoding sorts later wins, so every replica
that sees both writes keeps the same one:

```lisp
(assert (= (gun:ham 10 5 "b" 5 "a") :accept))
(assert (= (gun:ham 10 5 "a" 5 "b") :same))
(assert (= (gun:ham 10 4 "z" 5 "a") :stale))
(assert (= (gun:ham 10 11 "z" 5 "a") :future))
(assert (= (gun:ham 10 5 "a" nil nil) :accept))
```

The **encoding** is the text form of a value: `n:` for nil, `t:` and `f:` for
the booleans, `i:` and `d:` for integers and floats, `s:` for strings, `x:` and
hex for bytes, and `l:` and the soul for links. Its order is total and means
nothing else.

A state is a hybrid logical clock packed into one integer: the wall clock in
milliseconds, shifted left 16 bits, plus a counter. `gun:clock` builds one, and
its `:now-ms` replaces the wall clock. Each write ticks the clock, and a tick is
always after the tick before it and after every state the clock has observed:

```lisp
(def hlc (gun:clock :now-ms (fn [] 1000)))
(def first-tick (hlc:tick))
(assert (= first-tick (* 1000 65536)))
(assert (= (hlc:tick) (+ first-tick 1)))
(hlc:observe (* 2000 65536))
(assert (= (hlc:tick) (+ (* 2000 65536) 1)))
```

A database observes every state it loads from its store before it merges
them. A store written by a clock that ran ahead therefore loads without
`:future`, and the next write still wins. A local write ticks the clock, and a
tick is never after `now`. So a local merge never meets a future state, and a
`:future` verdict inside one raises
`{:error :gun-error :reason :future-state}`.

A peer whose clock runs ahead can send a state from the future. Such a field
waits until its state is due, and the clock never observes it while it waits.
A field from a peer that is due is observed, so the next local write wins
locally. [gun/mesh.md](gun/mesh.md) gives the steps.

## Stores

A store is a struct of two functions: `:read` takes a soul and answers its
fields, or `nil`, and `:write` takes a soul and the fields a merge accepted.
A field is a `[state value]` pair. The database reads a soul once, the first
time a chain reaches it, and keeps the node in memory after that.

`gun:memory-store` keeps nodes in the process. Two databases opened on one
memory store share it:

```lisp
(def store (gun:memory-store))
(gun:put (gun:get (gun:open :store store) "cfg") {:theme "dark"})
(assert (= (gun:once (gun:get (gun:open :store store) "cfg"))
           {"theme" "dark"}))
```

`gun:redis-store` keeps each node in one Redis hash named `<prefix>:<soul>`.
Each hash field is a key of the node, and its value is the state in decimal, a
space, and the encoding:

```text
HGETALL app:users/alice
1) "name"
2) "117425205471346688 s:Alice"
3) "boss"
4) "117425205471346688 l:users/bob"
```

`std/redis` holds its connection in a parameter that only `redis:with` binds.
A Redis store therefore works inside the caller's `redis:with`, and so do
fibers spawned there, which inherit the binding:

```lisp
(defn with-redis-db [prefix body]
  "Run body on a database kept in Redis on 127.0.0.1 under prefix."
  (let [redis ((import "std/redis"))
        gun ((import "std/gun") :redis redis)]
    (redis:with "127.0.0.1" 6379
      (fn [] (body gun (gun:open :store (gun:redis-store :prefix prefix)))))))
```

## Peers

`gun:serve` accepts peers on a TCP listener until the listener closes, so run
it in its own fiber. `gun:connect` dials a peer at a `ws://` URL. It returns
when each side has registered the other, and answers a function that closes the
connection. `gun:peers` answers the ids of the connected peers. A write on
either side reaches the other, so given the module that `gun-with-mesh` builds,
this function answers `{"text" "hello"}`:

```lisp
(defn peers-example [mesh-gun]
  "Join two databases over loopback, write on one, and read on the other."
  (let [listener (tcp/listen "127.0.0.1" 0)
        at (port/path listener)
        url (string "ws://127.0.0.1:"
                    (parse-int (slice at (+ 1 (string/find at ":")))) "/gun")
        server (mesh-gun:open)
        client (mesh-gun:open)]
    (ev/spawn (fn [] (mesh-gun:serve server listener)))
    (let [hang-up (mesh-gun:connect client url)]
      (defer
        (begin
          (hang-up)
          (port/close listener))
        (mesh-gun:put (mesh-gun:get client "note") {:text "hello"} :ack 5)
        (mesh-gun:once (mesh-gun:get server "note"))))))
```

Every peer keeps every write it hears, and forwards what it accepted to its
other peers. The first read that reaches a soul asks the peers for it, and
waits for their answers for `:wait` seconds at most. `gun:open` takes `:wait`,
which defaults to 1.

`gun:put` and `gun:set` return when the write has landed locally. Given
`:ack seconds`, they also wait for the first peer to ack. They raise
`{:error :gun-error :reason :no-ack}` when no peer acks in time, and
`:peer-error` when the peer's merge failed. The write has landed locally in
both cases.

A write that comes from a peer has no caller to raise to. So an error that
handling it raises goes to the function `gun:open` takes as `:on-error`, with
the error as its one argument. The default writes a line to stderr.

## Invariants

1. A `put` validates its whole value before it merges anything, so a rejected
   value writes nothing.
2. One lock per database covers each `put` from its plan to its last store
   write, and each merge of a peer's write. Each field therefore reaches the
   store in state order, even when the store yields between two writes.
3. A `put` notifies the `gun:on` functions after it releases the lock. A
   function can therefore write to the database it watches.
4. An error raised by a `gun:on` function propagates out of the local `put`
   that called it. The write has reached the store by then.
5. No fiber waits on the network while it holds the lock. Each send, and each
   wait for an answer or an ack, happens after the lock is released.
6. The walk that decides which `gun:on` functions to call never waits on the
   network.

## Running tests

The memory store needs nothing outside the process:

```bash
elle tests/lang/gun.lisp
```

The mesh needs the `plugin/hash` and `plugin/random` plugins, and joins its
databases over loopback:

```bash
elle tests/lang/gun-mesh.lisp
```

The Redis store needs Redis on 127.0.0.1:6379. The file also joins two
Redis-backed databases, so it needs the same plugins:

```bash
elle tests/lang/gun-redis.lisp
```
