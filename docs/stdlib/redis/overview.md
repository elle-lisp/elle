# redis

<!-- audited: 2026-11-12 -->

The submodules behind [redis.lisp](../../../lib/redis.lisp): the RESP codec, the ambient connection, and the commands.

[redis.md](../redis.md) holds what a caller of the module sees. This file holds
how the module divides. Each file's header says what it exports, and
`(doc name)` carries a function's arguments.

| File | Purpose |
|------|---------|
| [resp.lisp](../../../lib/redis/resp.lisp) | The RESP2 codec: a command's parts, a reply, and the connection a reply is read from |
| [connection.lisp](../../../lib/redis/connection.lisp) | The ambient connection: `*redis-conn*`, the lock beside it, `cmd`, `using`, `with`, `manager`, `pipeline`, and pub/sub over a connection |
| [keyspace.lisp](../../../lib/redis/keyspace.lisp) | String, key and SCAN commands |
| [collections.lisp](../../../lib/redis/collections.lisp) | Hash, list and set commands |
| [zset.lisp](../../../lib/redis/zset.lisp) | Sorted-set commands |
| [server.lisp](../../../lib/redis/server.lisp) | Server commands, transactions and Lua scripting |

A submodule takes the submodules it needs as arguments, so loading one by hand
follows the order [redis.lisp](../../../lib/redis.lisp) uses:

```lisp
(def resp ((import "std/redis/resp")))
(def connection ((import "std/redis/connection") :resp resp))
(def keyspace ((import "std/redis/keyspace") :connection connection :resp resp))
(def collections
  ((import "std/redis/collections") :connection connection :resp resp))
(def zset ((import "std/redis/zset") :connection connection))
(def server ((import "std/redis/server") :connection connection :resp resp))
(assert (= ["*1\r\n" "$4\r\n" "PING" "\r\n"] (resp:encode "PING")))
(assert (fn? keyspace:get))
(assert (fn? collections:hget))
(assert (fn? zset:zadd))
(assert (fn? server:ping))
```

[redis.lisp](../../../lib/redis.lisp) names every export once, in the struct at its
bottom, so a command keeps its public name whichever file defines it. The
codec carries its own checks as `:test`, which `(redis:test)` runs.
