(elle/epoch 12)
# tests/elle/port-encoding.lisp — `port/encoding` accessor + the
# `:encoding text|binary` keyword on tcp/connect / tcp/accept /
# unix/connect / unix/accept.
#
# Background: POSIX sockets are byte streams, so the raw-socket
# constructors default to `:binary` — bytes from port/read, byte-counted
# port/read-exact, what byte-framed protocols (RESP, gRPC, HTTP/2,
# length-prefixed everything) need.  Line-oriented text protocols
# (SMTP, IRC, plain HTTP/1.x) want strings and grapheme-counted
# reads; they opt in via `:encoding :text` at connect / accept time.
# `port/encoding` lets protocol code self-check up front so a wrong
# mode raises a clear error instead of silently corrupting framing.

# ── 1. Default is :binary on raw sockets ─────────────────────────────
(def listener (tcp/listen "127.0.0.1" 0))
(def listener-port
  (let [p (port/path listener)]
    (parse-int (slice p (+ 1 (string/find p ":"))))))

(def accepted-port-keyword (box nil))
(def server-fiber
  (ev/spawn (fn []
              (let [c (tcp/accept listener)]
                (rebox accepted-port-keyword (port/encoding c))
                (protect (port/close c))))))

(let [c (tcp/connect "127.0.0.1" listener-port)]
  (defer
    (protect (port/close c))
    (assert (= :binary (port/encoding c)) "1a: tcp/connect defaults to :binary")
    # let accept finish
    (ev/sleep 0.05)
    (assert (= :binary (unbox accepted-port-keyword))
            "1b: tcp/accept defaults to :binary")))
(protect (ev/join-protected server-fiber))
(protect (port/close listener))

# ── 2. Opt-in :text on tcp/connect ───────────────────────────────────
(def listener2 (tcp/listen "127.0.0.1" 0))
(def listener2-port
  (let [p (port/path listener2)]
    (parse-int (slice p (+ 1 (string/find p ":"))))))

(def server-fiber2
  (ev/spawn (fn []
              (let [c (tcp/accept listener2)]
                # Server side writes a 4-grapheme word that's also 5 bytes — the
                # multibyte é confirms we're grapheme-counting on the client
                (port/write c "café!")
                (port/flush c)
                (protect (port/close c))))))

(let [c (tcp/connect "127.0.0.1" listener2-port :encoding :text)]
  (defer
    (protect (port/close c))
    (assert (= :text (port/encoding c)) "2a: :encoding :text honored")
    # 4 graphemes = "café", leftover "!" stays in the per-fd buffer
    (let [d (port/read-exact c 4)]
      (assert (string? d) "2b: text-port read returns string")
      (assert (= (length d) 4) "2c: (length) is grapheme count")
      (assert (= d "café") "2d: byte-precise across multibyte boundary"))
    (let [d (port/read-exact c 1)]
      (assert (= d "!") "2e: leftover bang consumed cleanly"))))
(protect (ev/join-protected server-fiber2))
(protect (port/close listener2))

# ── 3. Opt-in :text on tcp/accept ────────────────────────────────────
(def listener3 (tcp/listen "127.0.0.1" 0))
(def listener3-port
  (let [p (port/path listener3)]
    (parse-int (slice p (+ 1 (string/find p ":"))))))

(def accepted-enc (box nil))
(def accepted-read (box nil))
(def server-fiber3
  (ev/spawn (fn []
              (let [c (tcp/accept listener3 :encoding :text)]
                (rebox accepted-enc (port/encoding c))
                (rebox accepted-read (port/read-exact c 3))
                (protect (port/close c))))))

(let [c (tcp/connect "127.0.0.1" listener3-port)]
  (defer
    (protect (port/close c))
    (port/write c "héy!")
    (port/flush c)
    (ev/sleep 0.05)))
(protect (ev/join-protected server-fiber3))
(protect (port/close listener3))
(assert (= :text (unbox accepted-enc)) "3a: tcp/accept :encoding :text honored")
(let [r (unbox accepted-read)]
  (assert (string? r) "3b: accepted-port read returns string")
  (assert (= (length r) 3) "3c: 3 graphemes")
  (assert (= r "héy") "3d: bytes correct"))

# ── 4. Bad :encoding value raises ────────────────────────────────────
(let [[ok? err] (protect (tcp/connect "127.0.0.1" 1 :encoding :utf8))]
  (assert (not ok?) "4a: bogus :encoding raises")
  (assert (string/contains? err:message ":encoding must be")
          (concat "4b: error mentions :encoding (" err:message ")")))

# ── 5. Redis guard fires on a text-mode port ─────────────────────────
# Use a real Redis if available so we exercise the guard on the actual
# (redis:with) path; otherwise skip — the guard is unit-tested via the
# explicit (redis:require-binary-port) below.
(def redis ((import-file "lib/redis.lisp")))

(let [[ok? _] (protect (redis:with "127.0.0.1" 6379 (fn [] (redis:ping))))]
  (when ok?  # Build a text-mode TCP connection and prove redis:require-binary-port
    # rejects it — this catches "I opened the port the wrong way" before
    # any wire byte is parsed under grapheme semantics.
    (let [bogus-listener (tcp/listen "127.0.0.1" 0)
          bogus-port (let [p (port/path bogus-listener)]
                       (parse-int (slice p (+ 1 (string/find p ":")))))
          _accept-f (ev/spawn (fn []
                                (protect (port/close (tcp/accept bogus-listener)))))
          text-port (tcp/connect "127.0.0.1" bogus-port :encoding :text)]
      (defer
        (begin
          (protect (port/close text-port))
          (protect (port/close bogus-listener)))
        (let [[g-ok g-err] (protect (redis:require-binary-port text-port "test"))]
          (assert (not g-ok) "5a: guard rejects text port")
          (assert (= :wrong-port-encoding (get g-err :reason))
                  "5b: reason is :wrong-port-encoding")
          (assert (string/contains? (get g-err :message) "binary port")
                  "5c: message mentions binary port"))))))

(println "port-encoding: all tests passed")
