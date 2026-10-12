# mesh

<!-- audited: 2026-10-11 -->

The peer protocol of std/gun: the messages, how a peer merges and forwards them, reads that ask the peers, acks, and deferral.

[gun.md](../gun.md) holds what a caller sees: `gun:serve`, `gun:connect`,
`gun:peers`, and the `:ack`, `:wait` and `:on-error` options. This file holds
the rules behind them. [wire.lisp](wire.lisp) builds and checks the messages,
[peer.lisp](peer.lisp) runs one connection, and [mesh.lisp](mesh.lisp) holds
the rules.

## Messages

Each message is one JSON object in one WebSocket text frame. A field travels
as `[state encoding]`, with the value in its `value:encode` form. JSON keeps
no trait table, so a link sent as a struct would arrive as a plain struct; its
encoding `l:<soul>` arrives as a link.

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
zero or more, or an encoding that `value:decode` rejects. Such a message raises
`{:error :gun-error :reason :bad-message}`.

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
peer has not loaded reaches the peer on the peer's first read. The caller dials
again after a drop; `gun:connect` does not redial.

**Sends.** `std/websocket` does not order the frames of two fibers that send on
one connection, so each peer has one send lock. A send that fails closes that
peer.

**Close.** A reader fiber is the only fiber that reads a connection, so closing
one aborts its reader fiber. The reader's cleanup removes the peer from
`gun:peers` and closes the socket. The other side reads the end of the stream
and removes the peer in turn.

## Merging what a peer sends

A **put** from a peer merges by these steps:

1. Under the database lock, load each soul from the store.
2. Split the fields into **due** and **later**: a field is later when its state
   is after the clock's `now`.
3. Observe the state of each due field, merge the due fields by HAM, and send
   the accepted diff to the store. Then release the lock.
4. Send the accepted diff, as a new put, to every peer except the one that sent
   it.
5. Call the `gun:on` functions whose value changed.
6. Ack the put.
7. Give the later fields to a fiber that sleeps until the earliest of them is
   due, and then merges them by steps 1 to 5. A field still later after that
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
goes to `:on-error`. An error raised by a `gun:on` function in step 5 goes to
`:on-error`, and the ack still says ok, because the write has landed.

A **local put** merges under the lock as before, and then sends the accepted
diff to every peer. A put that accepted nothing sends nothing.

## Reads

The first read that reaches a soul **fetches** it: it loads the soul from the
store, then sends a `get` to every peer and waits until each one answers or
leaves, for `:wait` seconds at most. An answer merges like a put from the peer
that sent it, by every step but the ack. Later reads of the soul are local, because gossip
and resync keep the soul current.

A soul is **pending** from the start of its fetch to its end, and **fetched**
after that. A second fiber that reads a pending soul waits for the first fetch
to end. With no peer connected, a fetch ends at once.

A **get** from a peer runs in a fiber of its own, so a slow get does not hold
up the puts behind it. For each soul it names that this database has neither
fetched nor started to fetch, it fetches the soul from every peer except the
asker. Then it answers with the nodes it holds. A soul that is pending answers
from local data at once, and what its fetch brings in reaches the asker
afterwards, as gossip.

**Pending ends a get cycle.** A database marks a soul pending before it
forwards a get for the soul. A get that comes back around a cycle meets the
pending soul and is answered at once, so no get travels further than one lap.

Which reads fetch:

- `gun:once` and `gun:on` fetch each node their chain reaches.
- `gun:put` and `gun:set` fetch each node on their path before they take the
  lock, so the plan follows the links that the mesh holds.
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
