# redis

<!-- audited: 2026-10-20 -->

Redis client speaking RESP2 over TCP, in one file of pure Elle with no plugin behind it.

The export struct at the bottom of [redis.lisp](redis.lisp) lists every
command, and `doc` on a command bound to a name carries its arguments.

```lisp
(def redis ((import "std/redis")))
```

## The connection

A connection is `{:port :framer}`: the TCP port the client writes, and a
[framer](framing.md) over it that answers the replies. `redis:connect`
opens one, and `redis:connection` builds one over a binary port the caller
opened. The framer holds the bytes after the reply it answered, so read the
port through the connection alone.

`*redis-conn*` is a parameter holding the current connection, and a command
reads it rather than taking it. `redis:with` binds it: it opens a
connection, binds the parameter and a lock beside it, runs a thunk, and
closes on the way out.

```lisp
(defn count-visit [host]
  "Increment the visit counter on the Redis server at host."
  (redis:with host 6379 (fn [] (redis:incr "visits"))))
```

`redis:manager` does not bind it. Its `:run` reconnects and retries a
thunk after an error, but it binds a parameter of its own, returned as
`:conn-param`, so a command inside the thunk answers `:no-connection`.

Pub/sub takes the connection directly: `redis:subscribe` and `redis:recv`
take it as an argument, because a subscribed connection can no longer serve
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

`redis:resp-read` answers the first four rows; the commands convert the
last three. It reads one reply from a connection and signals an error
reply, which `redis:resp-read-raw` returns as a struct instead:

```lisp
(with-temp-dir dir
  (let [path (path/join dir "replies")]
    (file/write path "$-1\r\n:42\r\n+PONG\r\n*2\r\n$1\r\na\r\n$2\r\nbc\r\n-ERR boom\r\n")
    (let [c (redis:connection (port/open-bytes path :read))]
      (assert (nil? (redis:resp-read c)))
      (assert (= 42 (redis:resp-read c)))
      (assert (= "PONG" (redis:resp-read c)))
      (let [reply (redis:resp-read c)]
        (assert (= reply ["a" "bc"]))
        (assert (= :array (type-of reply))))
      (assert (= :server-error (get (redis:resp-read-raw c) :reason)))
      (redis:close c))))
```

A bulk string is a view of the framer's chunk, so a reply costs no copy
however large it is. [framing.md](framing.md) says what a view keeps
alive.

## Invariants

1. `resp-read-raw` returns an error struct; `resp-read` signals it.
2. Bulk string lengths come from `string/size-of`, which counts bytes
   rather than graphemes.

## Running tests

The RESP codec tests itself and needs no server:

```lisp
(assert (redis:test))
```

The rest needs Redis on 127.0.0.1:6379:

```bash
elle tests/lang/redis.lisp
```
