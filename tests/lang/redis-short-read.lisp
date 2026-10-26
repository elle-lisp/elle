(elle/epoch 15)
# audited: 2026-10-21
# resp-read reassembles a bulk string that spans more than one TCP segment.
# lib/redis.md
#
# Background: port/read on a stream socket (TCP, Unix) answers "up to N
# bytes", the POSIX semantics, and io_uring does not read on past a short read
# on a stream socket (src/io/uring/resubmit.rs). So a bulk reply the kernel
# hands back across two recv() calls arrives in pieces. resp-read takes the
# bulk through framing:exact, which reads on until it holds the whole length.
#
# The counter-factual: a reader that took the bulk with one port/read would
# return truncated data, and leave the tail of the bulk and its trailing \r\n
# on the wire. The next resp-read would then read those bytes as a reply, and
# parse a chunk of payload as a RESP prefix.
#
# This test confirms the property even single-fiber callers depend on:
# a redis bulk reply round-trips byte-for-byte regardless of size.
#
# The threshold for short reads on loopback is roughly the kernel TCP
# recv buffer (default ~64 KiB on Linux). The test uses 200 KiB, so the kernel
# almost always splits the reply. Off-loopback the threshold is much
# smaller (1 MSS), but a single test value covers both regimes.
#
# Requires a live Redis on 127.0.0.1:6379.  Records a reasoned skip otherwise.

(def redis ((import "std/redis")))

(def value-size 200000)
(def n-rounds 200)

# Redis is shared infrastructure, so the key carries this process's pid. A
# fixed name would collide the way a fixed scratch filename does. A second run
# against the same server (another checkout, a rerun overlapping this one)
# would seed, read and delete the same key. The deletion would arrive as a nil
# reply the reader cannot tell from a short read.
(def test-key (string "test:redis-short-read:" (sys/pid) ":big"))

# A value whose bytes vary with position, so a reply spliced together from the
# wrong offsets fails the byte-for-byte comparison instead of matching by luck.
(defn build-value []
  (let [@buf @""
        @i 0]
    (while (< i value-size)
      (push buf (string (mod i 10)))
      (assign i (+ i 1)))
    (freeze buf)))

# The whole test body, as a thunk for redis:with.  It is a named function so
# the connection block stays shallow enough to read, and so a gated run never
# pays for build-value.
(defn run-short-read []
  (let [big-value (build-value)]
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
                (push fail-msgs (string "round " r ": get raised " val)))
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
        (eprintln "FAIL: redis-short-read detected " (length fail-msgs)
                  " corruption(s)")
        (let [@i 0]
          (while (and (< i (length fail-msgs)) (< i 10))
            (eprintln "  " (get fail-msgs i))
            (assign i (+ i 1))))
        (when (> (length fail-msgs) 10)
          (eprintln "  ... (" (- (length fail-msgs) 10) " more)"))
        (assert false
                "redis-short-read: bulk-string framing corrupted by short read")))))

# Gate on a live Redis: without one the runner records a skip that carries the
# reason.  Never (exit 0) — under the runner that would terminate the whole
# process mid-run and silently drop every later form.
(gate! (service-up? "127.0.0.1" 6379) "redis not running on 127.0.0.1:6379"
       (println "  redis: available")
       (redis:with "127.0.0.1" 6379 run-short-read) (println "")
       (println "redis-short-read test passed."))
