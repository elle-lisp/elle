(elle/epoch 12)
## tests/elle/port-shortwrite.lisp
##
## `port/write` writes every byte before it returns, and returns that count.
## A caller never loops on the return value.
##
## One write(2) transfers only what fits in the fd's send buffer at that
## moment, so a single syscall covers a large payload only by luck. Both
## backends therefore resubmit the unwritten tail until the payload is gone
## (see docs/io.md and the full-write invariant in src/io/AGENTS.md).
##
## The payload sizes below are chosen so one syscall provably cannot finish
## the job. Measured on loopback with a single write:
##   - a 4 KiB send buffer accepted 21,845 bytes of a 200,000-byte write
##   - a default send buffer accepted 2,588,923 bytes of an 8,000,000-byte write
## Measured against a remote host:
##   - a remote socket accepted 40,520 bytes of a 150,000-byte write
##
## Both assertions check the returned count AND the bytes the peer actually
## received, because the two fail independently: a backend that loops but
## reports only the last chunk breaks the count, and one that reports the full
## length without looping breaks the peer's tally.
##
## A regression here is quiet on small payloads — they fit in one syscall — and
## quiet on loopback, which absorbs megabytes. It shows up as a large request to
## a real remote host arriving truncated, the peer waiting for a body that never
## finishes, and the connection dying at the peer's idle timeout.
##
## Cases 1 and 2 test the primitive directly. Case 3 tests the same property end
## to end through `lib/tls.lisp`, which is the largest caller that relies on it:
## every one of its six sends hands a whole buffer to `port/write` and trusts the
## count, so a backend that stops looping breaks TLS before anything else. The
## two layers fail independently — a new TLS send path that bypasses `port/write`
## would pass cases 1 and 2 and fail case 3.
##
## `port-shortread-framing.lisp` covers the read direction.
## The thread-pool backend runs the same file via the `port_shortwrite_threadpool`
## pin in tests/integration/elle_scripts.rs (`--no-uring` is process-global).

## ── Helper ───────────────────────────────────────────────────────────

(defn listen-port [listener]
  "Return the port number of a listener bound to an ephemeral port."
  (let [parts (string/split (port/path listener) ":")]
    (parse-int (get parts (- (length parts) 1)))))

(defn write-once [payload-size sndbuf]
  "Write `payload-size` bytes in ONE port/write call and report the outcome.
   Returns [returned-count peer-received]. The peer drains continuously, so a
   write that loops to completion can always finish. Pass sndbuf 0 to leave
   the send buffer at its default."
  (let* [listener (tcp/listen "127.0.0.1" 0)
         port-num (listen-port listener)
         @received 0]
    ## Drain the connection until EOF and count every byte that arrives.
    (ev/spawn (fn []
                (let [conn (tcp/accept listener)]
                  (forever
                    (let [chunk (port/read conn 65536)]
                      (when (nil? chunk) (break))
                      (assign received (+ received (length chunk)))))
                  (port/close conn))))
    (let* [conn (if (> sndbuf 0)
                  (tcp/connect "127.0.0.1" port-num :sndbuf sndbuf :timeout 5000)
                  (tcp/connect "127.0.0.1" port-num :timeout 5000))
           returned (port/write conn (bytes (string/repeat "x" payload-size)))]
      ## Close the write side so the reader sees EOF, then let it finish.
      (port/close conn)
      (ev/sleep 0.5)
      (port/close listener)
      [returned received])))

## ── 1. A small send buffer truncates a modest write ──────────────────
##
## A 4 KiB send buffer makes the short write appear at 200 KB. This case is
## fast and needs no large payload.

(ev/run (fn []
          (let [[returned received] (write-once 200000 4096)]
            (assert (= returned 200000)
                    (concat "small sndbuf: port/write returned "
                            (string returned)
                            " of 200000 bytes — the primitive stopped early"))
            (assert (= received 200000)
                    (concat "small sndbuf: peer received " (string received)
                            " of 200000 bytes — the rest was never sent")))))

(println "  1. 200 KB through a 4 KiB send buffer: every byte written")

## ── 2. A default socket truncates a large write ──────────────────────
##
## The defect needs no socket options. A default send buffer on loopback
## accepts about 2.5 MB, so an 8 MB write stops early as well.

(ev/run (fn []
          (let [[returned received] (write-once 8000000 0)]
            (assert (= returned 8000000)
                    (concat "default sndbuf: port/write returned "
                            (string returned)
                            " of 8000000 bytes — the primitive stopped early"))
            (assert (= received 8000000)
                    (concat "default sndbuf: peer received " (string received)
                            " of 8000000 bytes — the rest was never sent")))))

(println "  2. 8 MB through a default send buffer: every byte written")

## ── 3. The same payload through a TLS connection ─────────────────────
##
## `lib/tls.lisp` sends whole ciphertext buffers with one `port/write` each and
## uses the count for nothing. An 8 MB record stream is far past what loopback's
## send buffer accepts at once, so the write is short at the syscall layer even
## though the module issues exactly one call — the case the module cannot see and
## therefore cannot handle itself.
##
## A dropped tail deadlocks both sides here rather than raising: the server waits
## on a read for bytes that were never sent, and the client waits on the reply the
## server will never write. The deadline below is what turns that deadlock into a
## failed assertion instead of a hung corpus run.

(def tls-payload 8000000)
(def tls-deadline 60)

## Try release build first, fall back to debug. Unlike `tls.lisp` this file does
## not gate on the plugin: cases 1 and 2 need no plugin and must still run.
(def tls-plugin
  (let [[ok? r] (protect (import-file "target/release/libelle_tls.so"))]
    (if ok?
      r
      (let [[ok2? r2] (protect (import-file "target/debug/libelle_tls.so"))]
        (if ok2? r2 nil)))))

(defn tls-write-once [tls cert-path key-path payload]
  "Send `payload` bytes over one loopback TLS connection and report
   [returned server-reply]. The server counts the plaintext it decrypts and
   reports the total back over the same connection, so the reply is the peer's
   own tally rather than anything the sender computed. Returns nil if the
   exchange did not finish within `tls-deadline` seconds."
  (let* [listener (tcp/listen "127.0.0.1" 0)
         port-num (listen-port listener)
         config (tls:server-config cert-path key-path)
         outcome @[nil nil]
         server (ev/spawn (fn []
                            (let [conn (tls:accept listener config)]
                              (defer
                                (tls:close conn)
                                (let [@total 0]
                                  (forever
                                    (let [chunk (tls:read conn 65536)]
                                      (when (nil? chunk) (break))
                                      (assign total (+ total (length chunk)))
                                      (when (>= total payload) (break))))
                                  (tls:write conn (string total "\n")))))))
         client (ev/spawn (fn []
                            (let [conn (tls:connect "127.0.0.1" port-num
                                  {:no-verify true})]
                              (defer
                                (tls:close conn)
                                (put outcome 0
                                     (tls:write conn
                                     (bytes (string/repeat "x" payload))))
                                (put outcome 1 (tls:read-line conn))))))
         finished (ev/timeout tls-deadline
                              (fn []
                                (ev/join server)
                                (ev/join client)
                                true))]
    (port/close listener)
    (if (nil? finished) nil [(get outcome 0) (get outcome 1)])))

(if (nil? tls-plugin)
  (println "  3. TLS: SKIPPED (elle-tls plugin not built)")
  (begin
    (def tls ((import-file "lib/tls.lisp") tls-plugin))
    ## The scratch dir is created only on the path that uses it, and removed
    ## below whether or not openssl produced a certificate.
    (def scratch (file/mktempdir))
    (def cert-path (path/join scratch "cert.pem"))
    (def key-path (path/join scratch "key.pem"))
    (let [gen-result (subprocess/system "openssl"
                                        ["req" "-x509" "-newkey" "rsa:2048"
                                        "-keyout" key-path "-out" cert-path
                                        "-days" "1" "-nodes" "-subj"
                                        "/CN=localhost"])]
      (if (not (= gen-result:exit 0))
        (println "  3. TLS: SKIPPED (openssl not available)")
        (begin
          (ev/run (fn []
                    (let [result (tls-write-once tls cert-path key-path
                          tls-payload)]
                      (assert (not (nil? result))
                              (concat "tls: the exchange did not finish in "
                                      (string tls-deadline)
                                      " s — a send dropped its tail and both sides "
                                      "blocked"))
                      (let [[returned reply] result]
                        (assert (= returned tls-payload)
                                (concat "tls: tls/write returned "
                                        (string returned) " of "
                                        (string tls-payload) " plaintext bytes"))
                        (assert (= reply (concat (string tls-payload) "\n"))
                                (concat "tls: server decrypted " (string reply)
                                        " rather than " (string tls-payload)
                                        " bytes — the rest was never sent"))))))
          (println "  3. 8 MB through a TLS connection: every byte written"))))
    (file/delete-dir-all scratch)))

(println "port-shortwrite: all tests passed")
