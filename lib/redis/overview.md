# redis

<!-- audited: 2026-10-20 -->

The submodules behind [redis.lisp](../redis.lisp): the RESP codec and its connection, the ambient connection, and the command wrappers.

[redis.md](../redis.md) holds what a caller of the module sees. This file holds
how the module divides. Each file's header says what it exports, and
`(doc name)` carries a function's arguments.

| File | Purpose |
|------|---------|
| [resp.lisp](resp.lisp) | The RESP2 codec: encode a command, read a reply, and the connection a reply is read from |
| [connection.lisp](connection.lisp) | The ambient connection: `*redis-conn*`, the lock beside it, `cmd`, `with`, `manager`, `pipeline`, and pub/sub over a connection |
| [keyspace.lisp](keyspace.lisp) | String, key and SCAN commands |
| [collections.lisp](collections.lisp) | Hash, list, set and sorted-set commands |
| [server.lisp](server.lisp) | Server commands, transactions and Lua scripting |

A submodule takes the submodules it needs as arguments, so loading one by hand
follows the order [redis.lisp](../redis.lisp) uses:

```lisp
(def resp ((import "std/redis/resp")))
(def connection ((import "std/redis/connection") :resp resp))
(def keyspace ((import "std/redis/keyspace") :connection connection :resp resp))
(def collections
  ((import "std/redis/collections") :connection connection :resp resp))
(def server ((import "std/redis/server") :connection connection :resp resp))
(assert (= (resp:encode "PING") "*1\r\n$4\r\nPING\r\n"))
(assert (fn? keyspace:get))
(assert (fn? collections:hget))
(assert (fn? server:ping))
```

[redis.lisp](../redis.lisp) names every export once, in the struct at its
bottom, so a command keeps its public name whichever file defines it. The
codec carries its own checks as `:test`, which `(redis:test)` runs.
