# mesh

<!-- audited: 2026-10-22 -->

How std/gun databases join over WebSocket: serve, connect, keepers and close, and the protocol of messages, merges, reads and acks.

[gun.md](../gun.md) holds the data model, and the `:websocket` that the module
needs for any of this. This file holds the functions a caller uses to join
peers, then the rules behind them. [wire.lisp](wire.lisp) builds and checks
the messages, and [peer.lisp](peer.lisp) runs one connection.
[wait.lisp](wait.lisp) records what a get or a put waits for,
[mesh.lisp](mesh.lisp) holds the rules, and [join.lisp](join.lisp) serves,
dials and keeps the connections.

## Joining

`gun:serve` accepts peers on a TCP listener until the listener closes, so run
it in its own fiber. `gun:connect` dials a peer at a `ws://` URL, and answers
a function that closes the connection. `gun:peers` answers the ids of the
connected peers. A write on either side reaches the other, so given a module
loaded with `:websocket`, this function answers `{"text" "hello"}`:

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

`gun:open` takes `:peers`, a list of `ws://` URLs, and keeps the database
joined to each one. For each URL it starts a **keeper**, a fiber that dials
the URL, and dials again each time the connection ends. `gun:open` returns at
once, so a read before the first connection answers from local data. When a
connection arrives, resync pulls every soul the database has loaded, and the
`gun:on` functions whose values changed are called.

A dial that fails gives its error to `:on-error`, and the keeper waits before
it dials again. `:redial [min max]` sets that wait in seconds, and defaults to
`[0.1 30]`. **Redial** below gives the rule. A `:redial` that is not two
positive numbers, with `min` no more than `max`, raises
`{:error :gun-error :reason :invalid-redial}`.

`gun:close` ends every keeper and closes every connected peer. It also closes
each peer that `gun:serve` registers after that, so close the listener too.
Local reads and writes still work, and a second `gun:close` does nothing.
After it, `gun:connect` raises `{:error :gun-error :reason :closed}`. A keeper
is a background fiber, and the end of the program ends it as it ends every
other. A program calls `gun:close` to end its keepers sooner:

```lisp
(def gun ((import "std/gun")))

(let [db (gun:open)
      cfg (gun:get db "cfg")]
  (gun:put cfg {:theme "dark"})
  (gun:close db)
  (gun:close db)
  (assert (empty? (gun:peers db)))
  (gun:put cfg {:font "mono"})
  (assert (= (gun:once cfg) {"theme" "dark" "font" "mono"})))
(let [[ok? err] (protect (gun:open :redial [5 1]))]
  (assert (= (get err :reason) :invalid-redial)))
```

`gun:open` takes `:wait`, the seconds a first read waits for the peers, 1 by
default (**Reads** below). `gun:put` and `gun:set` return when the write has
landed locally, and take `:ack` to wait for a peer as well (**Acks** below).

A write that comes from a peer has no caller to raise to. So an error that
handling it raises goes to the function `gun:open` takes as `:on-error`, with
the error as its one argument. The default writes a line to stderr.

## Messages

Each message is one JSON object in one WebSocket text frame. A field travels
as `[state encoding]`, with the value in its `value:encode` form. JSON keeps
no trait table, so a link sent as a struct would arrive as a plain struct; its
encoding `l:<soul>` arrives as a link. A field in a user space travels as
`[state encoding signature]`, with the signature in hex
([sea.md](sea.md)).

| Message | Meaning |
|---------|---------|
| `{"hi": id}` | The first message on a connection, from each side. `id` names the database |
| `{"id": m, "put": graph}` | A graph diff |
| `{"id": m, "get": [soul, ...]}` | Asks for whole nodes |
| `{"ack": m, "put": graph}` | The answer to the get `m`. It holds the nodes the peer has, and can be `{}` |
| `{"ack": m, "ok": true}` | The peer merged the put `m` |
| `{"ack": m, "err": reason}` | The merge of the put `m` raised. `reason` is the error's `:reason`, by name, or `error` when it has none |

A graph is an object of soul to node, and a node is an object of key to field:

```text
{"id":"5c0f…","put":{"users/alice":{"boss":[117425205471346688,"l:bob"],
                                    "name":[117425205471346688,"s:Alice"]}}}
```

A message that is not one of these does not decode. Neither does a soul that is
not a non-empty string, a key that is `_`, a state that is not an integer of
zero or more, an encoding that `value:decode` rejects, or a signature that is
not 128 lowercase hex characters. Such a message raises
`{:error :gun-error :reason :bad-message}`. Decoding checks the form of a
signature only; admission checks that it verifies.

Each hop sends messages of its own, with fresh ids, so an ack answers one hop
only. A message id comes from `uuid:v4`.

## Connections

The peer that dials sends `hi` first. The server registers the dialer, answers
`hi`, and then starts the resync. The dialer registers the server when the
answer arrives, starts its own resync, and `gun:connect` returns. So when
`gun:connect` returns, each side lists the other in `gun:peers`. When no answer
arrives within `:wait`, `gun:connect` closes the connection and raises
`{:error :gun-error :reason :peer-timeout}`.

**Resync.** Once a side registers a peer, it sends that peer one `get` for
every soul it has loaded. The answer carries the peer's fields for those souls,
read from its store when its memory lacks them. Both sides do this, so each
side pulls the souls it holds, and two databases that wrote while apart
converge when they meet again. A side needs no push: a soul it holds and the
peer has not loaded reaches the peer on the peer's first read.

**Redial.** `gun:connect` dials once, and a keeper dials as `gun:connect`
does, in a loop. After a dial that succeeds, the keeper waits for that peer to
leave. After a dial that
fails, it gives the dial's error to `:on-error`. Either way, it then waits for
its delay and dials again. The delay starts at `min` of `:redial`, doubles
after each failed dial up to `max`, and goes back to `min` after a dial that
succeeds. So a peer that drops is dialed again `min` seconds later, and an
unreachable peer is dialed once each `max` seconds at most.

**Closing a database.** Each peer has a latch that its close opens, and a
keeper waits on it. The database has a close latch, and a keeper waits for
its delay on that latch, under a timeout. `gun:close` opens the close latch,
so every keeper wakes at once and ends. A peer that registers after the
close, through `gun:serve` or through a dial that was under way, is closed as
it registers, and its hello is not answered. A keeper whose dial fails after
the close reports nothing.

**Sends.** `std/websocket` does not order the frames of two fibers that send on
one connection, so each peer has one send lock. A send that fails closes that
peer.

**Closing a connection.** A reader fiber is the only fiber that reads a
connection. Closing a connection removes the peer from `gun:peers` at once and
closes the socket, which ends the reader's pending read; `ws:close` would read
a frame itself, in a race with the reader. The other side reads the end of the
stream and removes the peer in turn.

## Merging what a peer sends

A **put** from a peer merges by these steps:

1. Under the database lock, pass every field of the message through admission
   ([sea.md](sea.md)). When one field fails, refuse the whole message, so
   nothing of it merges.
2. Load each soul from the store.
3. Split the fields into **due** and **later**: a field is later when its state
   is after the clock's `now`.
4. Observe the state of each due field, merge the due fields by HAM, and send
   the accepted diff to the store. Then release the lock.
5. Send the accepted diff, as a new put, to every peer except the one that sent
   it.
6. Call the `gun:on` functions whose value changed.
7. Ack the put.
8. Give the later fields to a fiber that sleeps until the earliest of them is
   due, and then merges them by steps 1 to 6. A field still later after that
   sleep waits again.

**HAM ends a forwarding loop.** A peer forwards only the diff that its merge
accepted. A write that a peer already holds merges as `:same`, accepts nothing,
and goes no further. Every write therefore crosses each connection a bounded
number of times, in a mesh of any shape, and a put needs no id table.

**A later field is never observed while it waits.** A peer whose clock runs
ahead therefore cannot move this clock ahead. A due field is observed, so the
next local write is after every state this database holds, and wins locally.

**Errors.** A message that does not decode goes to `:on-error` and is dropped.
The connection stays open. A put whose merge raises acks `err`, and the error
goes to `:on-error`; a put that admission refuses acks `err` with the reason
that admission gave. An error raised by a `gun:on` function in step 6 goes to
`:on-error`, and the ack still says ok, because the write has landed.

A **local put** merges under the lock as before, and then sends the accepted
diff to every peer. A put that accepted nothing sends nothing.

## Reads

The first read that reaches a soul **fetches** it: it loads the soul from the
store, then sends a `get` to every peer and waits until each one answers or
leaves, for `:wait` seconds at most. An answer merges like a put from the peer
that sent it, by every step but the ack. Later reads of the soul are local,
because gossip and resync keep the soul current.

A soul is **pending** from the start of its fetch to its end, and **fetched**
after that. A second fiber that reads a pending soul waits for the first fetch
to end. With no peer connected, a fetch ends at once.

A fetch that skipped a peer leaves its souls unfetched when it ends, so the
next read asks every peer. Only the fetch that a get from a peer starts skips
one, the asker. Without this rule, the get that a resync sends would mark the
asker's own souls fetched on the side that never asked the asker for them.

A **get** from a peer runs in a fiber of its own, so a slow get does not hold
up the puts behind it. For each soul it names that this database has neither
fetched nor started to fetch, it fetches the soul from every peer except the
asker. Then it answers with the nodes it holds. A soul that is pending answers
from local data at once, and what its fetch brings in reaches the asker
afterwards, as gossip.

A fetch that a get started skips that get's asker, so it brings in nothing
that peer holds. When a get from another peer meets a soul pending from such a
fetch, the database also sends the skipped peer a get for the soul, once per
fetch. It waits for no answer, which merges like any other, so the skipped
peer's fields reach the asker as gossip too.

**Pending ends a get cycle.** A database marks a soul pending before it
forwards a get for the soul. A get that comes back around a cycle meets the
pending soul and is answered at once, so no get travels further than one lap.

Which reads fetch:

- `gun:once` and `gun:on` fetch each node their chain reaches. A map fetches
  the nodes of all its members with one get.
- `gun:put`, `gun:set`, `gun:unset` and `gun:soul` fetch each node on their
  path before they take the lock, so the plan follows the links that the mesh
  holds.
- The walk that decides which `gun:on` functions to call never waits on the
  network. When it reaches a soul that nobody has fetched, it starts a fetch in
  a new fiber, and the answer calls the functions again.

## Acks

`(gun:put chain v :ack seconds)` waits for the first ack from a peer that the
put went to. An `ok` ack returns. An `err` ack raises
`{:error :gun-error :reason :peer-error :peer id :peer-reason r}`. When the put
went to no peer, it raises `{:error :gun-error :reason :no-ack}` at once. When
no ack arrives within `seconds`, or every peer it went to leaves first, it
raises `:no-ack` then. In every case, the write has landed locally.

## The lock and the network

Every merge and every store write happens under the database lock. Every send,
and every wait for an answer or an ack, happens outside it. A reader fiber that
waited on the network under the lock would hold up the reader that brings the
answer, so this rule is what keeps a mesh from stalling.
