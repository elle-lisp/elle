# redis

<!-- audited: 2026-09-14 -->

Redis client speaking RESP2 over TCP, in one file of pure Elle with no plugin behind it.

The export struct at the bottom of [redis.lisp](redis.lisp) lists every
command, and `(doc name)` carries each one's arguments.

## The connection is a bare port

There is no wrapper struct. `*redis-port*` is a parameter holding the
current connection, and a command reads it rather than taking it. Two
ways to bind it:

- `redis:with` opens a connection, binds the parameter, runs a thunk and
  closes on the way out.
- `redis:manager` reconnects after an error it does not judge terminal,
  and crashes on one it does.

Pub/sub is the exception: `redis:subscribe` and `redis:recv` take the
port directly, because a subscribed connection can no longer serve
ordinary commands.

## Value mapping

| Redis | Elle |
|-------|------|
| Nil bulk string (`$-1`) | `nil` |
| Integer reply | integer |
| Simple string | string |
| Array reply | immutable array |
| HGETALL | struct with string keys, not keywords |
| EXISTS, HEXISTS, SISMEMBER, EXPIRE | boolean |
| OK | `true` |

## Invariants

1. `resp-read-raw` returns an error struct; `resp-read` signals it.
2. Bulk string lengths come from `string/size-of`, which counts bytes
   rather than graphemes.

## Running tests

The RESP codec tests itself and needs no server:

```lisp
(def redis ((import "std/redis")))
(redis:test)                            # => true
```

The rest needs Redis on 127.0.0.1:6379:

```bash
elle tests/elle/redis.lisp
```
