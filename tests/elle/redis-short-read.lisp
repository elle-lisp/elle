(elle/epoch 12)
# tests/elle/redis-short-read.lisp — resp-read must reassemble bulk
# strings that span more than one TCP segment.
#
# Background: port/read on a stream socket (TCP, Unix) follows POSIX
# "up to N bytes" semantics — io_uring deliberately does *not* resubmit
# short reads on stream sockets (see src/io/uring.rs, the
# `if !is_stream` gate around the read-short-read resubmission path).
# So a single (port/read port (+ len 2)) for a bulk reply that the
# kernel hands back across two recv() calls returns truncated data and
# leaves the tail of the bulk plus its trailing \r\n on the wire.  The
# next resp-read then read-lines those leftover bytes as a "reply" and
# parses a chunk of payload as a RESP prefix — typically a digit or
# '-' that surfaces as "integer: cannot parse \"...,...,...\" as
# base-10 integer" or "unexpected RESP prefix: <byte>".
#
# This test confirms the property even single-fiber callers depend on:
# a redis bulk reply round-trips byte-for-byte regardless of size.
#
# The threshold for short reads on loopback is roughly the kernel TCP
# recv buffer (default ~64 KiB on Linux); we use 200 KiB so the kernel
# almost always splits the reply.  Off-loopback the threshold is much
# smaller (1 MSS), but a single test value covers both regimes.
#
# Pre-fix: at least one round trips a short read; the failing read
# either truncates the returned string or pollutes the next reply and
# raises an unexpected-prefix / cannot-parse error.
# Post-fix: 200 round trips all succeed with byte-exact echo.
#
# Requires a live Redis on 127.0.0.1:6379.  Skips silently otherwise.

(def redis ((import-file "lib/redis.lisp")))

(def value-size 200000)
(def n-rounds 200)
(def test-key "test:redis-short-read:big")

(let [[ok? _] (protect (redis:with "127.0.0.1" 6379 (fn [] (redis:ping))))]
  (when (not ok?)
    (eprintln "SKIP: Redis not available at 127.0.0.1:6379")
    (exit 0)))
(println "  redis: available")

(def big-value
  (let [@buf @""
        @i 0]
    (while (< i value-size)
      (push buf (string (mod i 10)))
      (assign i (+ i 1)))
    (freeze buf)))

(redis:with "127.0.0.1" 6379
            (fn []
              (redis:set test-key big-value)
              (println "  seeded " value-size "-byte value")

              (def @ok true)
              (def @fail-msgs @[])
              (let [@r 0]
                (while (< r n-rounds)
                  (let [[gok val] (protect (redis:get test-key))]
                    (cond
                      (not gok)
                        (begin
                          (assign ok false)
                          (push fail-msgs
                                (string "round " r ": get raised " val)))
                      (nil? val)
                        (begin
                          (assign ok false)
                          (push fail-msgs (string "round " r ": got nil")))
                      (not (= (string/size-of val) value-size))
                        (begin
                          (assign ok false)
                          (push fail-msgs
                                (string "round " r ": short reply, got "
                                        (string/size-of val) " want " value-size)))
                      (not (= val big-value))
                        (begin
                          (assign ok false)
                          (push fail-msgs
                                (string "round " r ": reply does not byte-match")))))
                  (assign r (+ r 1))))

              (redis:del test-key)

              (if ok
                (println "  " n-rounds " rounds × " value-size
                         "-byte get: all byte-exact")
                (begin
                  (eprintln "FAIL: redis-short-read detected "
                            (length fail-msgs) " corruption(s)")
                  (let [@i 0]
                    (while (and (< i (length fail-msgs)) (< i 10))
                      (eprintln "  " (get fail-msgs i))
                      (assign i (+ i 1))))
                  (when (> (length fail-msgs) 10)
                    (eprintln "  ... (" (- (length fail-msgs) 10) " more)"))
                  (assert false
                          "redis-short-read: bulk-string framing corrupted by short read")))))

(println "")
(println "redis-short-read test passed.")
