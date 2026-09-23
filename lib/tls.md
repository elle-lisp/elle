# tls

<!-- audited: 2026-09-23 -->

TLS 1.2 and 1.3 client and server: the `elle-tls` plugin runs the state machine, and Elle code moves every byte.

The export struct at the bottom of [tls.lisp](tls.lisp) lists what the
module offers, and `(doc name)` carries each function's arguments. The
module takes the plugin as its argument, `(import "plugin/tls")`:

```lisp
(defn tls-status-line [tls-plugin host]
  "Open a TLS connection to host:443 and return the first line of its answer."
  (let* [tls ((import "std/tls") tls-plugin)
         conn (tls:connect host 443)]
    (defer (tls:close conn)
      (tls:write conn (string "HEAD / HTTP/1.1\r\nHost: " host
                              "\r\nConnection: close\r\n\r\n"))
      (tls:read-line conn))))
```

A function that moves bytes yields, because it does I/O: `connect`,
`accept`, `read`, `read-line`, `read-all`, `write` and `close`.
`server-config` and `alpn-protocol` do no I/O. `lines`, `chunks` and
`writer` return a fiber, which yields when you resume it.

## Data flow

```
connect: tcp/connect → tls/client-state → handshake loop → tls-conn
accept:  tcp/accept  → tls/server-state → handshake loop → tls-conn

handshake loop: tls/process bytes → drain outgoing → complete? → read more

read:  plaintext buffer → port/read → tls/process → tls/read-plaintext
write: tls/write-plaintext → port/write
```

A `tls-conn` is transparent, and both fields are yours to read:

```text
{:tcp <port>       # the raw TCP connection
 :tls <tls-state>} # the plugin's state object
```

## Invariants

1. Every `tls/process` call leaves outgoing data that you must drain and
   send before you read again.
2. `tls:close` sends `close_notify`, then closes the TCP port. An error
   from `tls/close-notify` or from the write propagates before the port
   closes.
3. `tls:lines` and `tls:chunks` close the connection when they run out.

## Running tests

```bash
elle tests/elle/tls.lisp
```
