(elle/epoch 9)
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
  (let [[ok? r] (protect (import-file "target/release/libelle_tls.so"))]
    (if ok?
      [ok? r]
      (protect (import-file "target/debug/libelle_tls.so")))))

(when (not ok?)
  (print "SKIP: elle-tls plugin not built (run: cargo build -p elle-tls)\n")
  (exit 0))

## Extract the handshake-complete? primitive for use in assertions.
## (Must be accessed via plugin struct — not resolvable as a global name.)
(def handshake-complete? (get tls-plugin :handshake-complete?))

## Load the TLS stdlib, passing the plugin struct so it can close over
## the plugin primitives.
(def tls ((import-file "lib/tls.lisp") tls-plugin))

## ── Probe network access ──────────────────────────────────────────────────
## If we can't reach example.com:443, skip the network-dependent tests.

(def has-network
  (let [[ok? _] (protect (tls:connect "example.com" 443))]
    ok?))

(when has-network

## ── Chunk 4: handshake test ─────────────────────────────────────────────────

((fn []
  (let [conn (tls:connect "example.com" 443)]
    (assert (not (nil? conn:tcp)) "tls: conn:tcp must be a port")
    (assert (not (nil? conn:tls)) "tls: conn:tls must be a tls-state")
    (assert (handshake-complete? conn:tls) "tls: handshake must be complete")
    (port/close conn:tcp))))

(println "tls chunk 4: handshake test PASSED\n")

## ── Chunk 5a: HTTPS GET with tls/read-all ─────────────────────────────────

((fn []
  (let [conn (tls:connect "example.com" 443)]
    (defer (tls:close conn)
      (tls:write conn "GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")
      (let [body (string (tls:read-all conn))]
        (assert (> (length body) 0)
                "tls: read-all must return non-empty body")
        (assert (string/contains? body "Example Domain")
                "tls: response body must contain 'Example Domain'"))))))

(println "tls chunk 5a: HTTPS GET with read-all PASSED\n")

## ── Chunk 5b: tls/lines with stream/collect ───────────────────────────────

((fn []
  (let [conn (tls:connect "example.com" 443)]
    (tls:write conn "GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")
    (let [lines (stream/collect (stream/take 5 (tls:lines conn)))]
      (assert (> (length lines) 0) "tls: lines must yield at least one line")
      (assert (string? (first lines)) "tls: each line must be a string")))))

(println "tls chunk 5b: tls/lines with stream/collect PASSED\n")

## ── Chunk 5c: tls/chunks with stream/map ──────────────────────────────────

((fn []
  (let [conn (tls:connect "example.com" 443)]
    (tls:write conn "GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")
    (let [sizes (stream/collect
                   (stream/map length
                     (stream/take 3 (tls:chunks conn 1024))))]
      (assert (> (length sizes) 0) "tls: chunks must yield at least one chunk")
      (each sz in sizes
        (assert (> sz 0) "tls: each chunk must be non-empty"))))))

(println "tls chunk 5c: tls/chunks with stream/map PASSED\n")

) # end (when has-network)

(unless has-network
  (println "tls chunks 4-5: SKIPPED (no network access)\n"))

## ── Chunk 6: loopback echo server/client ──────────────────────────────────
##
## Generates a self-signed cert via openssl, starts a TLS echo server and a
## TLS client in the same the async scheduler, verifies the full round-trip.

## Generate test certificates. If openssl is not available or fails, skip.
(let [cert-path "/tmp/elle-tls-test.cert.pem"
      key-path  "/tmp/elle-tls-test.key.pem"]

  (let [gen-result
          (subprocess/system "openssl"
            ["req" "-x509" "-newkey" "rsa:2048"
             "-keyout" key-path
             "-out" cert-path
             "-days" "1" "-nodes"
             "-subj" "/CN=localhost"])]
    (if (not (= gen-result:exit 0))
      (println "tls chunk 6: SKIPPED (openssl not available)\n")

      (begin
        ## Shared mutable cell: the client fiber writes true here when done.
        ## Checked after the async scheduler to confirm the client fiber actually completed.
        (def loopback-ok @[false])

        ## Set up the listener before spawning fibers so the port is bound
        ## and the address is known before either fiber runs.
        (def listener (tcp/listen "127.0.0.1" 0))
        (def server-addr (port/path listener))

        ## Parse the ephemeral port from "127.0.0.1:PORT".
        (def server-port
          (let [parts (string/split server-addr ":")]
            (parse-int (get parts (- (length parts) 1)))))

        (def server-config (tls:server-config cert-path key-path))

        (def server-fiber
          (ev/spawn (fn []
            (let [conn (tls:accept listener server-config)]
              (defer (tls:close conn)
                (let [msg (tls:read-line conn)]
                  (when (not (nil? msg))
                    (let [trimmed (string/trim msg)]
                      (tls:write conn (string "echo: " trimmed "\n"))))))))))
        (def client-fiber
          (ev/spawn (fn []
            (let [conn (tls:connect "127.0.0.1" server-port {:no-verify true})]
              (defer (tls:close conn)
                (tls:write conn "hello\n")
                (let [response (tls:read-line conn)]
                  (assert (= response "echo: hello\n")
                          (string "tls loopback: expected \"echo: hello\\n\", got: "
                                  response))
                  (put loopback-ok 0 true)))))))
        (ev/join server-fiber)
        (ev/join client-fiber)

        (assert (get loopback-ok 0) "tls loopback: client fiber must complete and verify response")
        (port/close listener)
        (println "tls chunk 6: loopback echo test PASSED\n")))))

## ── Chunk 7: error cases ─────────────────────────────────────────────────────
##
## Tests that bad inputs fail with correct error kinds, not silently or with panics.
## Plugin primitives accessed via the tls-plugin struct.

## Extract primitives we need to call directly for error-path testing.
(def client-state-fn   (get tls-plugin :client-state))
(def process-fn        (get tls-plugin :process))
(def write-plaintext-fn (get tls-plugin :write-plaintext))
(def server-config-fn  (get tls-plugin :server-config))

## Error 1: empty hostname → :tls-error signal
(let [[ok? err] (protect (client-state-fn ""))]
  (assert (not ok?) "empty hostname: must signal an error")
  (assert (= (get err :error) :tls-error)
          (string "empty hostname: error kind must be :tls-error, got: "
                  (string (get err :error)))))

(println "tls chunk 7: empty hostname rejected ✓\n")

## Error 2: wrong type for tls/process (string instead of bytes) → :type-error signal
(let [state (client-state-fn "example.com")]
  (let [[ok? err] (protect (process-fn state "not-bytes"))]
    (assert (not ok?) "tls/process with string: must signal an error")
    (assert (= (get err :error) :type-error)
            (string "tls/process with string: must be :type-error, got: "
                    (string (get err :error))))))

(println "tls chunk 7: tls/process type-check ✓\n")

## Error 3: tls/write-plaintext before handshake → {:status :error} (SIG_OK, error in struct)
(let [state (client-state-fn "example.com")]
  (let [result (write-plaintext-fn state (bytes "hello"))]
    (assert (= result:status :error)
            (string "write-plaintext before handshake: result:status must be :error, got: "
                    (string result:status)))))

(println "tls chunk 7: write-plaintext before handshake returns error struct ✓\n")

## Error 4: tls/server-config with non-existent cert path → :io-error signal
(let [[ok? err] (protect (server-config-fn "/nonexistent/cert.pem" "/nonexistent/key.pem"))]
  (assert (not ok?) "invalid cert path: must signal an error")
  (assert (= (get err :error) :io-error)
          (string "invalid cert path: must be :io-error, got: "
                  (string (get err :error)))))

(println "tls chunk 7: invalid cert path rejected ✓\n")

## Error 5: tls/connect to a closed port → :io-error or :connect-error
## Port 19999 on 127.0.0.1 should have nothing listening.
## the async scheduler propagates errors out, so protect wraps the the async scheduler call.
(let [[ok? err] (protect ((fn []
                                    (tls:connect "127.0.0.1" 19999))))]
  (assert (not ok?) "connect to closed port: must signal an error")
  (assert (or (= (get err :error) :io-error)
              (= (get err :error) :connect-error)
              (= (get err :error) :tls-error))
          (string "connect to closed port: must be :io-error or :connect-error, got: "
                  (string (get err :error)))))

(println "tls chunk 7: connect to closed port rejected ✓\n")

## Error 6: tls/connect to a plain-HTTP port → :tls-error (handshake fails on non-TLS data)
## Requires network access.
(if has-network
  (begin
    (let [[ok? err] (protect ((fn []
                                        (tls:connect "example.com" 80))))]
      (assert (not ok?) "connect to plain HTTP port: must signal an error")
      (assert (or (= (get err :error) :tls-error)
                  (= (get err :error) :io-error)
                  (= (get err :error) :connect-error))
              (string "connect to plain HTTP port: must be :tls-error or :io-error, got: "
                      (string (get err :error)))))
    (println "tls chunk 7: connect to plain-HTTP port rejected ✓\n"))
  (println "tls chunk 7: connect to plain-HTTP port SKIPPED (no network)\n"))

(println "tls chunk 7: all error cases PASSED\n")
