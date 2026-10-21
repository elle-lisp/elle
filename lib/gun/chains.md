# chains

<!-- audited: 2026-10-21 -->

How a std/gun chain reads and writes: get, put, once, on and set, maps, key ranges, souls, links by chain, unset and content.

[gun.md](../gun.md) holds the data model that a chain reads and writes, and
the merge rule that decides each write. This file holds the chain functions.
`(doc name)` carries each function's arguments.

## Chains

A database is a chain, and `gun:get` extends a chain by one key. The first key
names a root node. Each later key names a field of the node before it. The
database itself names no node, so a `put` or a `once` on it raises
`{:error :gun-error :reason :no-soul}`. A root holds a node, so a `put` of
anything but a struct on a root raises `:not-a-node`:

```lisp
(def gun ((import "std/gun")))
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

## Maps

`gun:map` answers a **map chain**: a chain that reads each field of a node as
a **member**. `gun:get` extends a map chain past the map, and the keys after
it are read from each member. `once` on a map chain answers a struct of member
key to member value:

- A member that holds a link is read on from the node it links to.
- A member that holds any other value is that value when no keys follow the
  map. When keys follow, it is left out.
- A member whose read answers nil is left out.

A node with no member answers `{}`, and a chain that reaches no node before
the map answers `nil`:

```lisp
(def team (gun:get db "team"))
(gun:put team {:ann {:name "Ann" :age 31} :bob {:name "Bob" :age 27} :size 2})
(assert (= (gun:once (gun:get (gun:map team) "name"))
           {"ann" "Ann" "bob" "Bob"}))
(assert (= (get (gun:once (gun:map team)) "size") 2))
(assert (= (gun:once (gun:map (gun:get db "empty-team"))) nil))
```

`on` on a map chain calls its function with two arguments, the member's value
and its key. It calls the function at once for each member. After that, it
calls the function for each member whose value a write changed. A member
leaves when a write tombstones it, or when its read comes to answer nil, and
the function is then called with nil:

```lisp
(def calls @[])
(def stop-ages (gun:on (gun:get (gun:map team) "age")
                       (fn [v k] (push calls [k v]))))
(assert (= (length calls) 2) "one call for each member at once")
(gun:put (gun:get (gun:get team "bob") "age") 28)
(gun:put (gun:get team "ann") nil)
(stop-ages)
(assert (= (slice (freeze calls) 2) [["bob" 28] ["ann" nil]]))
```

The first read of a map fetches every member node from the peers with one
get. A map chain only reads: `put`, `set`, `unset` and `soul` on one raise
`{:error :gun-error :reason :map-chain}`.

## Key ranges

`gun:map` takes a range of member keys. A key passes when it starts with
`:prefix`, is at least `:from`, and is before `:to`. Keys compare as strings,
`:to` itself is left out, and a bound that is not given passes every key. So
two ranges that meet at one bound share no key. A bound that is not a string
raises `{:error :gun-error :reason :invalid-range}`:

```lisp
(def log (gun:get db "log"))
(gun:put log {"2026-01-03" "a" "2026-01-31" "b" "2026-02-01" "c"})
(assert (= (gun:once (gun:map log :from "2026-01" :to "2026-02"))
           {"2026-01-03" "a" "2026-01-31" "b"}))
(assert (= (gun:once (gun:map log :prefix "2026-02")) {"2026-02-01" "c"}))
```

A range filters the node that a read holds. It does not ask a peer for part of
a node.

## Souls and links by chain

`gun:soul` answers the soul that a `put` of a struct at a chain writes to. It
follows each link on the chain's path, reading from the peers as a `put`
does, and derives a child soul for each key after the last link:

```lisp
(assert (= (gun:soul team) "team"))
(assert (= (gun:soul (gun:get team "bob")) "team/bob"))
(gun:put (gun:get team "lead") (gun:link "team/bob"))
(assert (= (gun:soul (gun:get (gun:get team "lead") "pet")) "team/bob/pet"))
```

A `put` takes a chain wherever it takes a value, at the chain or inside a
struct, and writes a link to the chain's soul. A chain on another database
links by its soul all the same:

```lisp
(gun:put (gun:get team "ann") {:mentor (gun:get team "bob")})
(assert (= (gun:link-soul (get (gun:once (gun:get team "ann")) "mentor"))
           "team/bob"))
```

A chain carries the `:Gun` protocol, as a link does
([overview.md](overview.md)). Its `:kind` method answers `:chain`, and its
`:soul` method answers what `gun:soul` answers.

## Unset

`gun:unset` removes a member from a collection. It takes the collection's
chain and the member, as a chain or a link, and writes a tombstone to every
field of the collection's node that links to the member. It signs and acks as
a `put` does. When no field links to the member, it writes nothing:

```lisp
(def chores (gun:get db "chores"))
(def dishes (gun:set chores {:title "dishes"}))
(gun:set chores {:title "laundry"})
(gun:unset chores dishes)
(let [titles (gun:once (gun:get (gun:map chores) "title"))]
  (assert (= (length titles) 1))
  (assert (= (first (values titles)) "laundry")))
```

## Content

`gun:put-content` writes a value into the content node `#`, under the key
that `gun:content-key` answers for it: the SHA-256 hex of the value's
encoding. It takes any chain of the database, and `:ack` as a `put` does.
Its answer is the key. [sea.md](sea.md) holds the rules of `#`, and the
content node needs the module's `:crypto`:

```lisp
(defn content-example [sea-gun]
  "Write a value into the content node, and read it back by its key."
  (let [db (sea-gun:open)
        k (sea-gun:put-content db "hello")]
    (assert (= k "159051e9a9d711b993a56b707677ffc5cb84e01cbbd587db672c08e69e04a324"))
    (assert (= (sea-gun:content-key "hello") k))
    (sea-gun:once (sea-gun:get (sea-gun:get db "#") k))))

(let [[ok? crypto] (protect (import "plugin/crypto"))]
  (when ok?
    (assert (= (content-example ((import "std/gun") :crypto crypto)) "hello"))))
```
