## TLS library integration tests — Chunk 4: handshake only
##
## Requires network access (connects to example.com:443).
## Requires the elle-tls plugin to be built:
##   cargo build -p elle-tls          (debug)
##   cargo build --release -p elle-tls (release)
##
## Run with:
##   ./target/release/elle tests/elle/tls.lisp
##
## Note: Plugin primitives are not resolvable by name at compile time.
## They are accessed via the struct returned by import-file and closed
## over in lib/tls.lisp.

## Try release build first, fall back to debug.
(def [ok? tls-plugin]
  (let [[[ok? r] (protect (import-file "target/release/libelle_tls.so"))]]
    (if ok?
      [ok? r]
      (protect (import-file "target/debug/libelle_tls.so")))))

(when (not ok?)
  (display "SKIP: elle-tls plugin not built (run: cargo build -p elle-tls)\n")
  (exit 0))

## Extract the handshake-complete? primitive for use in assertions.
## (Must be accessed via plugin struct — not resolvable as a global name.)
(def handshake-complete? (get tls-plugin :handshake-complete?))

## Load the TLS stdlib, passing the plugin struct so it can close over
## the plugin primitives.
(def tls ((import-file "lib/tls.lisp") tls-plugin))

## ── Chunk 4: handshake test ─────────────────────────────────────────────────

(ev/run (fn []
  (let [[[ok? result] (protect
                        (tls:connect "example.com" 443))]]
    (assert ok? (concat "tls: connect to example.com:443 failed: " (string result)))
    (let [[conn result]]
      # Verify the tls-conn shape.
      (assert (not (nil? conn:tcp)) "tls: conn:tcp must be a port")
      (assert (not (nil? conn:tls)) "tls: conn:tls must be a tls-state")
      (assert (handshake-complete? conn:tls) "tls: handshake must be complete")
      # Clean up without trying to send data.
      (port/close conn:tcp)))))

(print "tls chunk 4: handshake test PASSED\n")

## ── Chunk 5a: HTTPS GET with tls/read-all ─────────────────────────────────

(ev/run (fn []
  (let [[conn (tls:connect "example.com" 443)]]
    (defer (tls:close conn)
      (tls:write conn "GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")
      (let [[body (string (tls:read-all conn))]]
        (assert (> (length body) 0)
                "tls: read-all must return non-empty body")
        (assert (string/contains? body "Example Domain")
                "tls: response body must contain 'Example Domain'"))))))

(print "tls chunk 5a: HTTPS GET with read-all PASSED\n")

## ── Chunk 5b: tls/lines with stream/collect ───────────────────────────────

(ev/run (fn []
  (let [[conn (tls:connect "example.com" 443)]]
    # Note: tls/close is called by tls/lines when the stream is exhausted.
    (tls:write conn "GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")
    (let [[lines (stream/collect (stream/take 5 (tls:lines conn)))]]
      (assert (> (length lines) 0) "tls: lines must yield at least one line")
      (assert (string? (first lines)) "tls: each line must be a string")))))

(print "tls chunk 5b: tls/lines with stream/collect PASSED\n")

## ── Chunk 5c: tls/chunks with stream/map ──────────────────────────────────

(ev/run (fn []
  (let [[conn (tls:connect "example.com" 443)]]
    (tls:write conn "GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")
    (let [[sizes (stream/collect
                   (stream/map length
                     (stream/take 3 (tls:chunks conn 1024))))]]
      (assert (> (length sizes) 0) "tls: chunks must yield at least one chunk")
      (each sz in sizes
        (assert (> sz 0) "tls: each chunk must be non-empty"))))))

(print "tls chunk 5c: tls/chunks with stream/map PASSED\n")
