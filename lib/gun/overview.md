# gun

<!-- audited: 2026-10-14 -->

The submodules behind [gun.lisp](../gun.lisp): states, values, fields, HAM, the graph, stores, SEA, the core and the mesh.

[gun.md](../gun.md) holds what a caller of the module sees.
[mesh.md](mesh.md) holds the peer protocol, and [sea.md](sea.md) holds the
rules of user spaces and encryption. This file holds how the module divides.
Each file's header says what it exports, and `(doc name)` carries a function's
arguments.

| File | Purpose |
|------|---------|
| [state.lisp](state.lisp) | The hybrid logical clock, and the only comparison of two states |
| [value.lisp](value.lisp) | What a value may be, links, and the encoding that stores and orders values |
| [field.lisp](field.lisp) | A field of a node: its state, its value, and its signature in a user space. Every other file builds and reads a field through it |
| [ham.lisp](ham.lisp) | The HAM verdict on one incoming field against the current one |
| [graph.lisp](graph.lisp) | Souls, the in-memory nodes, the merge that answers a diff, and the flattening of a nested struct |
| [store.lisp](store.lisp) | The memory store and the Redis store, behind one interface |
| [wire.lisp](wire.lisp) | The peer messages: build them, write them as JSON, and read and check them |
| [sea.lisp](sea.lisp) | The rules of [sea.md](sea.md): key pairs, user spaces, signing, admission and encryption |
| [core.lisp](core.lisp) | One database: its graph, store, clock, lock, subscriptions and peers; loading, the chain walk, the merges and notify |
| [peer.lisp](peer.lisp) | One WebSocket connection: the hello, the reader fiber, the send lock and close |
| [mesh.lisp](mesh.lisp) | The rules of [mesh.md](mesh.md): fetch, forward, acks, deferral and resync |

A submodule takes the submodules it needs as arguments, so loading one by hand
follows the order [gun.lisp](../gun.lisp) uses. The exception is
`std/gun/field`, which takes no argument and which each submodule imports
for itself:

```lisp
(def state ((import "std/gun/state")))
(def value ((import "std/gun/value")))
(def ham ((import "std/gun/ham") :state state :value value))
(def graph ((import "std/gun/graph") :state state :value value :ham ham))
(def store ((import "std/gun/store") :state state :value value))
(def wire ((import "std/gun/wire") :value value :graph graph))
(def sea ((import "std/gun/sea") :value value))
(def core ((import "std/gun/core") :state state :value value :graph graph :sea sea))
(def peer ((import "std/gun/peer") :wire wire))
(def mesh ((import "std/gun/mesh") :core core :wire wire :peer peer))
(assert (= (value:encode (value:link "users/alice")) "l:users/alice"))
(assert (= (graph:child-soul "x" "a/b") "x/a%2Fb"))
(assert (= (sea:space-of "~ab/c") "~ab"))
(assert (nil? (sea:space-of "users/alice")))
(assert (= (wire:encode (wire:get "m1" ["users"])) "{\"get\":[\"users\"],\"id\":\"m1\"}"))
```

A link is a struct `{:soul s}` whose trait table carries the `:Gun`
protocol. Its `:kind` method answers `:link`, and its `:soul` method answers
`s`. [value.lisp](value.lisp) builds the table, and
[traits.md](../../docs/traits.md) says how a library's own protocol works:

```lisp
(let [l (value:link "users/alice")
      methods (get (traits l) :Gun)]
  (assert (= ((get methods :kind) l) :link))
  (assert (= ((get methods :soul) l) "users/alice")))
```

`std/gun/store` takes `std/redis` as `:redis`, and builds a Redis store only
when it has one. `std/gun/peer` takes `std/websocket` as `:websocket`, and
serves or dials only when it has one. `std/gun/sea` takes the crypto and
random plugins as `:crypto` and `:random`, and makes pairs, signs, checks and
encrypts only when it has both.
