(elle/epoch 15)
# audited: 2026-10-20
# The RESP reader with no server: a bulk reply across the framer's chunks, and the client's self-tests.
# lib/redis.md

(def redis ((import-file "lib/redis.lisp")))

# ── A reply across the framer's chunks (no Redis needed) ────────────────────
#
# A connection reads replies through std/framing, so a bulk string longer
# than one chunk is one reply however the chunks fall, and the reply after
# it starts where the bulk ended. The counter-factual is a client that read
# the length line and the bulk through separate port reads and parsed the
# bytes between them as the next reply.

(with-temp-dir dir
               (let [path (path/join dir "bulk")
                     payload (string/repeat "0123456789" 2000)]
                 (file/write path (string "$20000\r\n" payload "\r\n+OK\r\n"))
                 (let [c (redis:connection (port/open-bytes path :read)
                       :size 4096)]
                   (assert (= payload (redis:resp-read c))
                           "a 20000-byte bulk string read through 4096-byte chunks")
                   (assert (= "OK" (redis:resp-read c)) "and the reply after it")
                   (let [[ok? err] (protect (redis:resp-read c))]
                     (assert (not ok?) "the end of the stream is an error")
                     (assert (= :unexpected-eof (get err :reason))
                             "named unexpected-eof"))
                   (redis:close c))))
(println "  a bulk string across chunks: ok")

# ── RESP self-tests (no Redis needed) ───────────────────────────────────────

(println "Running RESP self-tests...")
(redis:test)
(println "RESP self-tests passed.")
