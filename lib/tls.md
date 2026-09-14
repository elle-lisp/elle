# tls

<!-- audited: 2026-09-14 -->

TLS 1.2 and 1.3 client and server: the `elle-tls` plugin runs the state machine, and Elle code moves every byte.

The export struct at the bottom of [tls.lisp](tls.lisp) lists what the
module offers, and `(doc name)` carries each function's arguments. Every
one of them yields, because every one of them does I/O.

## Data flow

```
connect: tcp/connect → tls/client-state → handshake loop → tls-conn
accept:  tcp/accept  → tls/server-state → handshake loop → tls-conn

handshake loop: tls/process bytes → drain outgoing → complete? → read more

read:  plaintext buffer → port/read → tls/process → tls/read-plaintext
write: tls/encrypt → port/write
```

A `tls-conn` is transparent, and both fields are yours to read:

```lisp
{:tcp <port>       # the raw TCP connection
 :tls <tls-state>} # the plugin's state object
```

## Invariants

1. Every `tls/process` call leaves outgoing data that you must drain and
   send before you read again.
2. `tls:close` closes the TCP port even when `close_notify` fails.
3. `tls:lines` and `tls:chunks` close the connection when they run out.

## Running tests

```bash
elle tests/elle/tls.lisp
```
