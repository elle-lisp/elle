(elle/epoch 11)
# tests/elle/port-read-exact.lisp — `port/read-exact` semantics.
#
# `port/read` follows POSIX "up to N" semantics and never resubmits
# short reads on stream sockets (per the gate in src/io/uring.rs).
# `port/read-exact` is the strict variant: it loops in the runtime
# until exactly N bytes have arrived, returning nil only on EOF
# before N.  This test exercises both the file-port (small enough
# that one read suffices) and stream-socket (large enough to force
# the kernel to hand it back in pieces, exposing the resubmission
# path) cases, plus the EOF-before-N → nil case.

(def small-text "hello world goodbye")

(spit "/tmp/elle-port-read-exact-small" small-text)

# ── 1. Text-mode file port: N is graphemes, result is a string of (length N).
#      ASCII case — bytes and graphemes coincide, but the contract is
#      grapheme-counted regardless.
(let [p (port/open "/tmp/elle-port-read-exact-small" :read)]
  (defer
    (port/close p)
    (let [d (port/read-exact p 5)]
      (assert (string? d) "1a: text port returns string")
      (assert (= (length d) 5) "1b: (length d) is grapheme count, = N")
      (assert (= d "hello") "1c: bytes match"))
    (let [d (port/read-exact p 6)]
      (assert (= (length d) 6) "1d: second read continues")
      (assert (= d " world") "1e: bytes match"))))

# ── 2. EOF before N → nil.  Note: count is graphemes here, and we ask for
#      more graphemes than the file has, regardless of byte size.
(let [p (port/open "/tmp/elle-port-read-exact-small" :read)]
  (defer
    (port/close p)
    (let [d (port/read-exact p (+ (length small-text) 100))]
      (assert (nil? d) "2a: EOF before N -> nil (not partial)"))))

# ── 3. Zero count returns empty (string on text port, no kernel I/O needed).
(let [p (port/open "/tmp/elle-port-read-exact-small" :read)]
  (defer
    (port/close p)
    (let [d (port/read-exact p 0)]
      (assert (= (length d) 0) "3a: zero count returns empty"))))

# ── 3b. Multi-byte UTF-8 grapheme: a single 'é' is 2 bytes but 1 grapheme.
#      port/read-exact 1 must reassemble both bytes and return the string "é"
#      of length 1.  Plain byte-counted code would split the codepoint.
(spit "/tmp/elle-port-read-exact-utf8" "café")
(let [p (port/open "/tmp/elle-port-read-exact-utf8" :read)]
  (defer
    (port/close p)
    (let [d (port/read-exact p 3)]
      (assert (= (length d) 3) "3b1: 3 graphemes from \"café\"")
      (assert (= d "caf") "3b2: first three graphemes are 'caf'"))
    (let [d (port/read-exact p 1)]
      (assert (= (length d) 1) "3b3: one more grapheme")
      (assert (= d "é") "3b4: fourth grapheme is the multi-byte 'é'"))))

# ── 3c. Over-read leftover stays for the next call.  After
#       (port/read-exact p 1) on "café", the second byte of 'é' must NOT
#       leak into subsequent reads.  Use a deliberately-mismatched read
#       size so any byte-vs-grapheme confusion would surface.
(spit "/tmp/elle-port-read-exact-utf8b" "café!")
(let [p (port/open "/tmp/elle-port-read-exact-utf8b" :read)]
  (defer
    (port/close p)
    (let [a (port/read-exact p 1)
          b (port/read-exact p 2)
          c (port/read-exact p 2)]
      (assert (= a "c") "3c1: c")
      (assert (= b "af") "3c2: af")
      (assert (= c "é!") "3c3: é! (multi-byte 'é' assembled, '!' after)"))))

# ── 3d. read-line over-read followed by read-exact.  port/read-line
#       reads more bytes than the line it returned (everything up to the
#       first '\n') and stashes the trailing bytes in the per-fd buffer.
#       The next port/read-exact must consume from that stash first,
#       split at the Nth grapheme, and stash the remainder back.  This
#       is the path the completion-side grapheme split exists for —
#       without it the read-line over-read leaks extra graphemes into
#       the read-exact result.
(spit "/tmp/elle-port-read-exact-mixed" "header\nbody-café-trailer\n")
(let [p (port/open "/tmp/elle-port-read-exact-mixed" :read)]
  (defer
    (port/close p)
    (let [hdr (port/read-line p)]
      (assert (= hdr "header") "3d1: read-line returns first line"))
    (let [chunk (port/read-exact p 4)]
      (assert (= (length chunk) 4)
              (concat "3d2: 4 graphemes, got " (string (length chunk)) " ("
                      chunk ")"))
      (assert (= chunk "body") "3d3: chunk = 'body'"))
    (let [chunk (port/read-exact p 1)]
      (assert (= chunk "-") "3d4: next read sees the '-' that was buffered"))
    (let [chunk (port/read-exact p 4)]
      (assert (= chunk "café") "3d5: multi-byte assembled across stash"))))

# ── 4. TCP loopback, count large enough to span TCP segments ──────────
# The kernel TCP recv buffer default is ~64 KiB on Linux loopback; a
# value past that almost always forces a short read on a single
# port/read call.  port/read-exact must loop to assemble the whole
# payload, byte-for-byte.

(def value-size 100000)
# Build the payload by doubling a period-10 ASCII pattern (O(log n)
# concats) rather than a 200 000-iteration per-byte push loop, which
# dominated the suite runtime.  Sent over a binary port, so the bytes
# are '0'..'9' = 48..57; built a bit past value-size and the reader
# takes exactly value-size, leaving the rest unread.
(def big-bytes
  (let [@s "0123456789"
        @len 10]
    (while (< len value-size)
      (assign s (concat s s))
      (assign len (* len 2)))
    s))

(def listener (tcp/listen "127.0.0.1" 0))
(def server-port
  (let [path (port/path listener)]
    (parse-int (slice path (+ 1 (string/find path ":"))))))

(def server-fiber
  (ev/spawn (fn []
              (let [client (tcp/accept listener)]
                (port/write client big-bytes)
                (port/flush client)
                (port/close client)))))

(let [sock (tcp/connect "127.0.0.1" server-port)]
  (defer
    (protect (port/close sock))
    (let [d (port/read-exact sock value-size)]
      (assert (not (nil? d))
              "4a: port/read-exact past the recv buffer returns non-nil")
      (assert (= (length d) value-size)
              (concat "4b: got " (string (length d)) " bytes, want "
                      (string value-size)))  # Spot-check bytes at the boundaries and around the expected
      # short-read split points.  d is bytes (TCP is Binary encoding);
      # walking every index would dominate the test runtime in the
      # VM-only path, so we sample.  '0'..'9' as bytes is 48+(i mod 10).
      (let [check-at @[0 1 2 100 1000 65535 65536 65537 (- value-size 1)]
            @j 0
            @mismatch nil]
        (while (and (nil? mismatch) (< j (length check-at)))
          (let [i (get check-at j)
                expected (+ 48 (mod i 10))]
            (when (not (= (get d i) expected))
              (assign
                mismatch
                (string "byte " i ": got " (get d i) " want " expected))))
          (assign j (+ j 1)))
        (assert (nil? mismatch) (concat "4c: " (or mismatch "")))))))

(protect (port/close listener))
(protect (ev/join-protected server-fiber))

# ── 5. Large TEXT read-exact canary.  On a text port N counts grapheme
# clusters, not bytes — with multibyte content the byte count far
# exceeds N, so the runtime must size its read buffer for bytes
# (graphemes * worst-case UTF-8 width) and chunk the kernel reads at the
# page size while still returning exactly N graphemes.  A regression in
# the buffer sizing or the >64 KiB chunked-resubmit path surfaces here
# as a short, nil, or garbled string.  Content alternates 'é' (2 bytes)
# and 'a' (1 byte), so bytes ≈ 1.5 × graphemes.
# Build "éa…" of exactly n graphemes by doubling (O(log n) concats —
# a per-grapheme loop at this size would dominate the suite runtime).
# n must be 2 × a power of two so doubling the 2-grapheme base lands on
# n exactly, no grapheme-unsafe slice needed.
(defn make-text [n]
  (let [@s "éa"
        @len 2]
    (while (< len n)
      (assign s (concat s s))
      (assign len (* len 2)))
    s))

(defn text-roundtrip [n label]
  (let [payload (make-text n)
        l (tcp/listen "127.0.0.1" 0)
        lport (let [path (port/path l)]
                (parse-int (slice path (+ 1 (string/find path ":")))))
        srv (ev/spawn (fn []
                        (let [client (tcp/accept l :encoding :text)]
                          (port/write client payload)
                          (port/flush client)
                          (port/close client))))]
    (let [sock (tcp/connect "127.0.0.1" lport :encoding :text)]
      (defer
        (protect (port/close sock))
        (let [d (port/read-exact sock n)]
          (assert (string? d) (concat label "a: returns a string"))
          (assert (= (length d) n)
                  (concat label "b: got " (string (length d))
                          " graphemes, want " (string n)))
          (assert (= d payload)
                  (concat label "c: content matches byte-for-byte")))))
    (protect (port/close l))
    (protect (ev/join-protected srv))))

# Moderate: ~3 KiB of bytes — fits one read, no resubmit.
(text-roundtrip 2048 "5")
# Over-large: 65536 graphemes ≈ 98 KiB of bytes — past the 64 KiB
# loopback recv buffer and the MAX_READ_CHUNK cap, forcing chunked
# resubmission across the page boundary.
(text-roundtrip 65536 "6")

(println "port-read-exact: all tests passed")
