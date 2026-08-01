(elle/epoch 12)
# tests/elle/redis-race.lisp — fiber-concurrent redis:pipeline must not
# interleave RESP framing on the wire.
#
# Background: redis:with binds *redis-port* via parameterize.  When the
# enclosing program spawns concurrent fibers (e.g., http2:serve handler
# fibers), every fiber sees the same port.  Without a mutex, pipelines
# from different fibers can interleave writes and reads — the RESP
# parser then tries to parse the middle of a bulk-string payload as
# the next length prefix and produces a parse error (the
# "integer: cannot parse \"<csv-of-floats>\" as base-10 integer" the
# grace polish-orchestrator was hitting under load).
#
# This test runs M fibers in parallel; each does a pipeline of N MGET
# commands against keys with a fiber-distinguishing prefix.  If
# framing is intact, every fiber sees exactly its own values back.
# If framing is corrupted, fibers see foreign values or RESP errors
# and the test fails.
#
# Requires a live Redis on 127.0.0.1:6379.  Skips silently otherwise.

(def redis ((import-file "lib/redis.lisp")))

# Redis is shared infrastructure, so the key prefix carries this process's pid.
# A fixed prefix collides the way a fixed scratch filename does: a second run
# against the same server — another checkout, a rerun overlapping this one —
# seeds, reads and deletes the same keys, and each run sees the other's
# deletions as its own corrupted framing.  The pid makes each run's keyspace
# its own, so a collision cannot be mistaken for the interleaving under test.
(def key-prefix (string "test:race:" (sys/pid) ":"))

(def n-fibers 8)
(def n-rounds 50)
(def n-keys-per-round 16)

# Skip if Redis isn't there.
(let [[ok? _] (protect (redis:with "127.0.0.1" 6379 (fn [] (redis:ping))))]
  (when (not ok?)
    (eprintln "SKIP: Redis not available at 127.0.0.1:6379")
    (exit 0)))
(println "  redis: available")

(redis:with "127.0.0.1" 6379
            (fn []
              # Seed: each fiber gets its own N keys with a known value so the
              # pipeline read can verify byte-for-byte what came back.
              (let [@f 0]
                (while (< f n-fibers)
                  (let [@k 0]
                    (while (< k n-keys-per-round)
                      (redis:set (string key-prefix f ":" k)
                                 (string "value-" f "-" k))
                      (assign k (+ k 1))))
                  (assign f (+ f 1))))
              (println "  seeded " (* n-fibers n-keys-per-round) " keys")

              # Run M fibers, each doing N rounds of a multi-key pipeline.
              # Each fiber records pass/fail in a shared atom and we tally at the
              # end.  Without the lock, any fiber seeing wrong values, RESP-level
              # errors, or a pipeline-length mismatch flips :ok to false.
              (def @ok true)
              (def @fail-msgs @[])
              (def @done-count 0)

              (let [@f 0]
                (while (< f n-fibers)
                  (let [fiber-idx f]
                    (ev/spawn (fn []
                                (let [@r 0]
                                  (while (< r n-rounds)
                                    (let [cmds @[]
                                      @k 0]
                                      (while (< k n-keys-per-round)
                                        (push cmds
                                        (list "GET"
                                        (string key-prefix fiber-idx ":" k)))
                                        (assign k (+ k 1)))
                                      (let [[pok? results] (protect (apply redis:pipeline
                                        (->list cmds)))]
                                        (if (not pok?)
                                          (begin
                                            (assign ok false)
                                            (push fail-msgs
                                            (string "fiber " fiber-idx " round "
                                            r ": pipeline raised " results)))
                                          (begin
                                            # Verify length and per-key value.
                                            (if (not (= (length results)
                                              n-keys-per-round))
                                              (begin
                                                (assign ok false)
                                                (push fail-msgs
                                                (string "fiber " fiber-idx
                                                " round " r
                                                ": pipeline returned "
                                                (length results)
                                                " replies, want "
                                                n-keys-per-round)))
                                              (let [@kk 0]
                                                (while (< kk n-keys-per-round)
                                                  (let [want (string "value-"
                                                    fiber-idx "-" kk)
                                                    got (get results kk)]
                                                    (when (not (= got want))
                                                      (assign ok false)
                                                      (push fail-msgs
                                                      (string "fiber " fiber-idx
                                                      " round " r " key " kk
                                                      ": want=" want " got="
                                                      (if (string? got)
                                                        (string "\"" got "\"")
                                                        got)))))
                                                  (assign kk (+ kk 1)))))))))
                                    (assign r (+ r 1))))
                                (assign done-count (+ done-count 1)))))
                  (assign f (+ f 1))))

              # Wait for all fibers to finish (cooperative — yield until counter hits target).
              (while (< done-count n-fibers) (ev/sleep 0.05))

              # Cleanup: delete only the keys we created.
              (let [@f 0]
                (while (< f n-fibers)
                  (let [@k 0]
                    (while (< k n-keys-per-round)
                      (redis:del (string key-prefix f ":" k))
                      (assign k (+ k 1))))
                  (assign f (+ f 1))))

              (if ok
                (println "  " n-fibers " fibers × " n-rounds " rounds × "
                         n-keys-per-round " keys: all framing intact")
                (begin
                  (eprintln "FAIL: redis-race detected " (length fail-msgs)
                            " corruption(s)")
                  (let [@i 0]
                    (while (and (< i (length fail-msgs)) (< i 10))
                      (eprintln "  " (get fail-msgs i))
                      (assign i (+ i 1))))
                  (when (> (length fail-msgs) 10)
                    (eprintln "  ... (" (- (length fail-msgs) 10) " more)"))
                  (assert false
                          "redis-race: pipeline framing corrupted under concurrent fibers")))))

(println "")
(println "redis-race test passed.")
