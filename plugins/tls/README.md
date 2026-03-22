# elle-tls

TLS support for Elle via rustls. Provides TLS 1.2 and 1.3 for both client
and server.

## Installation

Build the plugin:

```bash
cargo build -p elle-tls --release
```

Load it in Elle:

```lisp
(import-file "target/release/libelle_tls.so")
(def tls ((import-file "lib/tls.lisp")))
```

## Quick start

### HTTPS client

```lisp
(ev/run
  (fn []
    (let [[conn (tls:connect "example.com" 443)]]
      (defer (tls:close conn)
        (tls:write conn "GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")
        (println (string (tls:read-all conn)))))))
```

### Stream processing

```lisp
(ev/run
  (fn []
    (let [[conn (tls:connect "api.example.com" 443)]]
      (defer (tls:close conn)
        (tls:write conn "GET /data HTTP/1.1\r\nHost: api.example.com\r\nConnection: close\r\n\r\n")
        (stream/for-each println (tls:lines conn))))))
```

### TLS server

```lisp
(ev/run
  (fn []
    (let [[config (tls:server-config "cert.pem" "key.pem")]
          [listener (tcp/listen "0.0.0.0" 8443)]]
      (forever
        (let [[conn (tls:accept listener config)]]
          (ev/spawn
            (fn []
              (defer (tls:close conn)
                (let [[line (tls:read-line conn)]]
                  (tls:write conn (concat "echo: " line)))))))))))
```

## API

### Connection

| Function | Returns | Description |
|----------|---------|-------------|
| `tls:connect host port [opts]` | tls-conn | Connect to TLS server |
| `tls:accept listener config` | tls-conn | Accept TLS connection |
| `tls:server-config cert key` | tls-server-config | Build server config |
| `tls:close conn` | nil | Send close_notify, close TCP |

### Data transfer

| Function | Returns | Description |
|----------|---------|-------------|
| `tls:read conn n` | bytes or nil | Read up to n bytes |
| `tls:read-line conn` | string or nil | Read a line |
| `tls:read-all conn` | bytes | Read until EOF |
| `tls:write conn data` | int | Encrypt and send |

### Streams

| Function | Returns | Description |
|----------|---------|-------------|
| `tls:lines conn` | coroutine | Yields lines |
| `tls:chunks conn n` | coroutine | Yields byte chunks |
| `tls:writer conn` | coroutine | Write-side stream |

### Options for `tls:connect`

```lisp
{:no-verify  false      # skip cert verification (dev only)
 :ca-file    nil        # path to custom CA bundle
 :client-cert nil       # path to client cert PEM
 :client-key  nil}      # path to client key PEM
```

## Compatibility matrix

| Operation | Works? | Note |
|-----------|--------|------|
| `tls:read`, `tls:write`, `tls:close` | ✓ | Direct call, async |
| `tls:lines`, `tls:chunks`, `tls:writer` | ✓ | Returns coroutine |
| `stream/map f (tls:lines c)` | ✓ | Coroutine composition |
| `stream/collect (tls:lines c)` | ✓ | Coroutine consumption |
| `port/lines conn` | ✗ | conn is not a Port |
| `port/read conn n` | ✗ | conn is not a Port |
