(elle/epoch 9)
# Socket options tests — :sndbuf, :rcvbuf, :nodelay, :keepalive on connect primitives
#
# Tests verify that socket option keywords are accepted, correctly validated,
# and produce working connections. Rust-level tests (uring::tests) verify
# the actual setsockopt/getsockopt round-trip on the fd.

# ── Helpers ──────────────────────────────────────────────────────────

(defn echo-server [listener]
  "Accept one connection, read a chunk, write it back, close."
  (ev/spawn (fn []
              (let [conn (unix/accept listener :timeout 5000)
                    data (port/read conn 65536)]
                (port/write conn data)
                (port/close conn)))))

(defn tcp-echo-server [listener]
  "Accept one connection, read a chunk, write it back, close."
  (ev/spawn (fn []
              (let [conn (tcp/accept listener :timeout 5000)
                    data (port/read conn 65536)]
                (port/write conn data)
                (port/close conn)))))

(defn tcp-port [listener]
  "Extract the numeric port from a listener's path (e.g. '127.0.0.1:8080')."
  (let [parts (string/split (port/path listener) ":")]
    (parse-int (get parts (- (length parts) 1)))))

# ── 1. unix/connect :sndbuf — basic roundtrip ────────────────────────

(let [path "/tmp/elle-test-sockopt-sndbuf.sock"
      listener (unix/listen path)]
  (echo-server listener)
  (let [conn (unix/connect path :sndbuf 1048576 :timeout 5000)]
    (port/write conn "hello-sndbuf")
    (let [resp (string (port/read conn 1024))]
      (assert (= resp "hello-sndbuf") "unix :sndbuf roundtrip"))
    (port/close conn))
  (port/close listener))

# ── 2. unix/connect :rcvbuf — basic roundtrip ────────────────────────

(let [path "/tmp/elle-test-sockopt-rcvbuf.sock"
      listener (unix/listen path)]
  (echo-server listener)
  (let [conn (unix/connect path :rcvbuf 1048576 :timeout 5000)]
    (port/write conn "hello-rcvbuf")
    (let [resp (string (port/read conn 1024))]
      (assert (= resp "hello-rcvbuf") "unix :rcvbuf roundtrip"))
    (port/close conn))
  (port/close listener))

# ── 3. tcp/connect :sndbuf — basic roundtrip ─────────────────────────

(let [listener (tcp/listen "127.0.0.1" 0)
      port (tcp-port listener)]
  (tcp-echo-server listener)
  (let [conn (tcp/connect "127.0.0.1" port :sndbuf 1048576 :timeout 5000)]
    (port/write conn "hello-tcp-sndbuf")
    (let [resp (string (port/read conn 1024))]
      (assert (= resp "hello-tcp-sndbuf") "tcp :sndbuf roundtrip"))
    (port/close conn))
  (port/close listener))

# ── 4. tcp/connect :nodelay — basic roundtrip ────────────────────────

(let [listener (tcp/listen "127.0.0.1" 0)
      port (tcp-port listener)]
  (tcp-echo-server listener)
  (let [conn (tcp/connect "127.0.0.1" port :nodelay true :timeout 5000)]
    (port/write conn "hello-nodelay")
    (let [resp (string (port/read conn 1024))]
      (assert (= resp "hello-nodelay") "tcp :nodelay roundtrip"))
    (port/close conn))
  (port/close listener))

# ── 5. tcp/connect :keepalive — basic roundtrip ──────────────────────

(let [listener (tcp/listen "127.0.0.1" 0)
      port (tcp-port listener)]
  (tcp-echo-server listener)
  (let [conn (tcp/connect "127.0.0.1" port :keepalive true :timeout 5000)]
    (port/write conn "hello-keepalive")
    (let [resp (string (port/read conn 1024))]
      (assert (= resp "hello-keepalive") "tcp :keepalive roundtrip"))
    (port/close conn))
  (port/close listener))

# ── 6. tcp/connect :rcvbuf — option works on TCP too ─────────────────

(let [listener (tcp/listen "127.0.0.1" 0)
      port (tcp-port listener)]
  (tcp-echo-server listener)
  (let [conn (tcp/connect "127.0.0.1" port :rcvbuf 1048576 :timeout 5000)]
    (port/write conn "hello-tcp-rcvbuf")
    (let [resp (string (port/read conn 1024))]
      (assert (= resp "hello-tcp-rcvbuf") "tcp :rcvbuf roundtrip"))
    (port/close conn))
  (port/close listener))

# ── 7. unix/connect :keepalive — option works on Unix too ────────────

(let [path "/tmp/elle-test-sockopt-keepalive-unix.sock"
      listener (unix/listen path)]
  (echo-server listener)
  (let [conn (unix/connect path :keepalive true :timeout 5000)]
    (port/write conn "hello-unix-keepalive")
    (let [resp (string (port/read conn 1024))]
      (assert (= resp "hello-unix-keepalive") "unix :keepalive roundtrip"))
    (port/close conn))
  (port/close listener))

# ── 8. Combined — :sndbuf + :timeout on unix ─────────────────────────

(let [path "/tmp/elle-test-sockopt-combined.sock"
      listener (unix/listen path)]
  (echo-server listener)
  (let [conn (unix/connect path :sndbuf 1048576 :timeout 5000)]
    (port/write conn "combined-test")
    (let [resp (string (port/read conn 1024))]
      (assert (= resp "combined-test") "unix combined :sndbuf :timeout"))
    (port/close conn))
  (port/close listener))

# ── 9. All four options on tcp/connect ────────────────────────────────

(let [listener (tcp/listen "127.0.0.1" 0)
      port (tcp-port listener)]
  (tcp-echo-server listener)
  (let [conn (tcp/connect "127.0.0.1"
                          port
                          :sndbuf 1048576
                          :rcvbuf 524288
                          :nodelay true
                          :keepalive true
                          :timeout 5000)]
    (port/write conn "all-four-options")
    (let [resp (string (port/read conn 1024))]
      (assert (= resp "all-four-options") "tcp all four options combined"))
    (port/close conn))
  (port/close listener))

# ── 10. Error: :sndbuf with string value ──────────────────────────────

(let [[ok? err] (protect ((fn [] (unix/connect "/tmp/x.sock" :sndbuf "foo"))))]
  (assert (not ok?) ":sndbuf string value signals type error"))

# ── 11. Error: :sndbuf with negative value ────────────────────────────

(let [[ok? err] (protect ((fn [] (unix/connect "/tmp/x.sock" :sndbuf -1))))]
  (assert (not ok?) ":sndbuf negative value signals error"))

# ── 12. Error: :sndbuf with zero ──────────────────────────────────────

(let [[ok? err] (protect ((fn [] (unix/connect "/tmp/x.sock" :sndbuf 0))))]
  (assert (not ok?) ":sndbuf zero signals error"))

# ── 13. Error: unknown keyword ────────────────────────────────────────

(let [[ok? err] (protect ((fn [] (unix/connect "/tmp/x.sock" :bogus 1))))]
  (assert (not ok?) "unknown keyword :bogus signals error"))

# ── 14. Error: :nodelay with non-boolean ──────────────────────────────

(let [[ok? err] (protect ((fn [] (tcp/connect "127.0.0.1" 1234 :nodelay 42))))]
  (assert (not ok?) ":nodelay with int signals type error"))

# ── 15. Error: :keepalive with string ─────────────────────────────────

(let [[ok? err] (protect ((fn [] (tcp/connect "127.0.0.1" 1234 :keepalive "yes"))))]
  (assert (not ok?) ":keepalive with string signals type error"))

# ── 16. Error: :rcvbuf with float ─────────────────────────────────────

(let [[ok? err] (protect ((fn [] (unix/connect "/tmp/x.sock" :rcvbuf 3.14))))]
  (assert (not ok?) ":rcvbuf with float signals type error"))

# ── 17. Error: odd keyword count ──────────────────────────────────────

(let [[ok? err] (protect ((fn [] (unix/connect "/tmp/x.sock" :sndbuf))))]
  (assert (not ok?) "odd keyword count signals arity error"))

# ── 18. Error: non-keyword key ───────────────────────────────────────

(let [[ok? err] (protect ((fn [] (unix/connect "/tmp/x.sock" 42 1048576))))]
  (assert (not ok?) "non-keyword key signals type error"))

# ── 19. Large unix write — :sndbuf enables 300KB single write ────────
#
# Without an enlarged send buffer, a single port/write of 300KB to a Unix
# socket may block (default kernel buffer ~208KB). With :sndbuf 2MB the
# write completes. The server reads all data and reports the byte count
# back, proving the full payload traversed the socket.

(defn large-read-server [listener]
  "Accept one connection, read all data, write back the byte count, close."
  (ev/spawn (fn []
              (let [conn (unix/accept listener :timeout 10000)
                    data (port/read-all conn)
                    count (string (length data))]
                (port/write conn count)
                (port/close conn)))))

(let [path "/tmp/elle-test-sockopt-large.sock"
      listener (unix/listen path)
      size 307200
      payload (string/repeat "A" size)]
  (large-read-server listener)
  (let [conn (unix/connect path :sndbuf 2097152 :timeout 10000)]
    (port/write conn payload)
    (unix/shutdown conn :write)
    (let [resp (string (port/read-all conn))]
      (assert (= resp (string size))
              "large unix write: server received all bytes"))
    (port/close conn))
  (port/close listener))

# ── 20. unix/accept :sndbuf — server-side buffer ─────────────────────
#
# The server accepts with :sndbuf 2MB, then writes back 300KB.
# Without enlarged send buffer on the accepted socket, the server's
# write can deadlock against the default ~208KB kernel buffer.

(defn large-write-server [listener size]
  "Accept with :sndbuf, read all, write back the full payload."
  (ev/spawn (fn []
              (let [conn (unix/accept listener :sndbuf 2097152 :timeout 10000)
                    data (port/read-all conn)]
                (port/write conn data)
                (port/close conn)))))

(let [path "/tmp/elle-test-sockopt-accept-sndbuf.sock"
      listener (unix/listen path)
      size 307200
      payload (string/repeat "B" size)]
  (large-write-server listener size)
  (let [conn (unix/connect path :sndbuf 2097152 :rcvbuf 2097152 :timeout 10000)]
    (port/write conn payload)
    (unix/shutdown conn :write)
    (let [resp (port/read-all conn)]
      (assert (= (length resp) size) "accept :sndbuf: full 300KB echo"))
    (port/close conn))
  (port/close listener))

# ── 21. tcp/accept :sndbuf — server-side buffer on TCP ───────────────

(let [listener (tcp/listen "127.0.0.1" 0)
      port (tcp-port listener)]
  (ev/spawn (fn []
              (let [conn (tcp/accept listener :sndbuf 1048576 :timeout 5000)
                    data (port/read conn 65536)]
                (port/write conn data)
                (port/close conn))))
  (let [conn (tcp/connect "127.0.0.1" port :timeout 5000)]
    (port/write conn "accept-tcp-sndbuf")
    (let [resp (string (port/read conn 1024))]
      (assert (= resp "accept-tcp-sndbuf") "tcp/accept :sndbuf roundtrip"))
    (port/close conn))
  (port/close listener))

# ── 22. accept error: unknown keyword ─────────────────────────────────

(let [listener (unix/listen "/tmp/elle-test-sockopt-accept-err.sock")]
  (let [[ok? err] (protect ((fn [] (unix/accept listener :bogus 1))))]
    (assert (not ok?) "unix/accept unknown keyword signals error"))
  (port/close listener))

# ── 23. accept error: bad :sndbuf type ────────────────────────────────

(let [listener (unix/listen "/tmp/elle-test-sockopt-accept-err2.sock")]
  (let [[ok? err] (protect ((fn [] (unix/accept listener :sndbuf "big"))))]
    (assert (not ok?) "unix/accept :sndbuf string signals type error"))
  (port/close listener))

(println "socket-options: all 23 tests passed")
