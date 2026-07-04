(elle/epoch 12)
# tests/elle/redis.lisp — Redis integration tests
#
# Requires a live Redis on 127.0.0.1:6379.
# All tests run within a single connection; the test cleans up only its own
# "test:redis:*" keys via SCAN+DEL so it never touches db0 (or any db) as a
# whole.  Each tests/elle/redis*.lisp file owns a distinct "test:<name>:*"
# sub-namespace so the parallel test runner can fire them concurrently
# against a shared Redis without one wiping another's keys mid-run.

(def redis ((import-file "lib/redis.lisp")))

# Delete only keys created by this test (prefix "test:redis:"), so we never run
# FLUSHDB / FLUSHALL against a shared Redis. Safe to call when no keys match.
# COUNT 1000: SCAN pages the whole keyspace and MATCH filters each page, so the
# server-default page size (10) costs one round trip per ~10 ambient keys. On a
# shared Redis with tens of thousands of keys that is thousands of round trips
# per call — this file calls it four times — enough to trip the smoke timeout.
(defn clear-test-keys []
  (let [keys (redis:scan-all redis:scan :match "test:redis:*" :count 1000)]
    (when (not (empty? keys)) (apply redis:del keys))))

# RESP self-tests (no Redis needed)
(println "Running RESP self-tests...")
(redis:test)
(println "RESP self-tests passed.")

# Gate the connection block on a live Redis: if it isn't reachable, the
# (redis:with …) form below emits a loud :gated (skip with a reason) instead of
# connecting and failing. Never (exit 0) — under the runner that would terminate
# the whole process mid-run and silently drop every later form.
(gate! (service-up? "127.0.0.1" 6379) "redis not running on 127.0.0.1:6379"
       (redis:with "127.0.0.1" 6379
                   (fn []

                     # ================================================================
                     # 1. Integration tests
                     # ================================================================

                     (assert (= (redis:ping) "PONG") "ping")
                     (println "  ping: ok")

                     (assert (= (redis:echo "hello") "hello") "echo")
                     (println "  echo: ok")

                     (clear-test-keys)

                     # ── String commands ─────────────────────────────────────────────

                     (assert (= (redis:set "test:redis:k1" "v1") true) "set")
                     (assert (= (redis:get "test:redis:k1") "v1") "get")
                     (assert (nil? (redis:get "test:redis:nonexistent"))
                             "get nil")

                     (assert (= (redis:set "test:redis:nx" "first" :nx true)
                                true) "set nx first")
                     (assert (= (redis:get "test:redis:nx") "first")
                             "get nx first")

                     (redis:set "test:redis:counter" "10")
                     (assert (= (redis:incr "test:redis:counter") 11) "incr")
                     (assert (= (redis:decr "test:redis:counter") 10) "decr")
                     (assert (= (redis:incrby "test:redis:counter" 5) 15)
                             "incrby")
                     (assert (= (redis:decrby "test:redis:counter" 3) 12)
                             "decrby")

                     (redis:set "test:redis:str" "hello")
                     (redis:append "test:redis:str" " world")
                     (assert (= (redis:get "test:redis:str") "hello world")
                             "append")
                     (assert (= (redis:strlen "test:redis:str") 11) "strlen")

                     (redis:mset "test:redis:m1" "a" "test:redis:m2" "b")
                     (let [vals (redis:mget "test:redis:m1" "test:redis:m2"
                           "test:redis:nonexistent")]
                       (assert (= (get vals 0) "a") "mget 0")
                       (assert (= (get vals 1) "b") "mget 1")
                       (assert (nil? (get vals 2)) "mget nil"))

                     (assert (= (redis:setnx "test:redis:setnx" "val") true)
                             "setnx new")
                     (assert (= (redis:setnx "test:redis:setnx" "val2") false)
                             "setnx exists")

                     (redis:mset "test:redis:m1" "a" "test:redis:m2" "b")
                     (let [vals (redis:mget "test:redis:m1" "test:redis:m2"
                           "test:redis:nonexistent")]
                       (assert (= (get vals 0) "a") "mget 0")
                       (assert (= (get vals 1) "b") "mget 1")
                       (assert (nil? (get vals 2)) "mget nil"))

                     (assert (= (redis:setnx "test:redis:setnx" "val") true)
                             "setnx new")
                     (assert (= (redis:setnx "test:redis:setnx" "val2") false)
                             "setnx exists")

                     (assert (= (redis:exists "test:redis:k1") true)
                             "exists true")
                     (assert (= (redis:exists "test:redis:nonexistent") false)
                             "exists false")

                     (redis:set "test:redis:exp" "val")
                     (assert (= (redis:expire "test:redis:exp" 100) true)
                             "expire")
                     (let [ttl (redis:ttl "test:redis:exp")]
                       (assert (> ttl 0) "ttl positive"))
                     (assert (= (redis:persist "test:redis:exp") true) "persist")
                     (assert (= (redis:ttl "test:redis:exp") -1)
                             "ttl after persist")

                     (assert (= (redis:exists "test:redis:k1") true)
                             "exists true")
                     (assert (= (redis:exists "test:redis:nonexistent") false)
                             "exists false")

                     (redis:set "test:redis:rename" "val")
                     (assert (= (redis:rename "test:redis:rename"
                                "test:redis:renamed") true) "rename")
                     (assert (= (redis:get "test:redis:renamed") "val")
                             "get renamed")

                     (assert (>= (redis:del "test:redis:k1" "test:redis:renamed")
                                 1) "del")
                     (assert (= (redis:exists "test:redis:k1") false)
                             "exists after del")

                     (redis:set "test:redis:rename" "val")
                     (assert (= (redis:rename "test:redis:rename"
                                "test:redis:renamed") true) "rename")
                     (assert (= (redis:get "test:redis:renamed") "val")
                             "get renamed")

                     (assert (>= (redis:del "test:redis:k1" "test:redis:renamed")
                                 1) "del")
                     (assert (= (redis:exists "test:redis:k1") false)
                             "exists after del")

                     (redis:hset "test:redis:hash" "name" "Alice")
                     (redis:hset "test:redis:hash" "age" "30")

                     (assert (= (redis:hget "test:redis:hash" "name") "Alice")
                             "hget")
                     (assert (nil? (redis:hget "test:redis:hash" "missing"))
                             "hget nil")
                     (assert (= (redis:hexists "test:redis:hash" "name") true)
                             "hexists true")
                     (assert (= (redis:hexists "test:redis:hash" "missing")
                                false) "hexists false")

                     (let [h (redis:hgetall "test:redis:hash")]
                       (assert (= (get h "name") "Alice") "hgetall name")
                       (assert (= (get h "age") "30") "hgetall age"))

                     (assert (= (redis:hget "test:redis:hash" "name") "Alice")
                             "hget")
                     (assert (nil? (redis:hget "test:redis:hash" "missing"))
                             "hget nil")
                     (assert (= (redis:hexists "test:redis:hash" "name") true)
                             "hexists true")
                     (assert (= (redis:hexists "test:redis:hash" "missing")
                                false) "hexists false")

                     (redis:hmset "test:redis:hm" "a" "1" "b" "2" "c" "3")
                     (let [vals (redis:hmget "test:redis:hm" "a" "c" "missing")]
                       (assert (= (get vals 0) "1") "hmget 0")
                       (assert (= (get vals 1) "3") "hmget 1")
                       (assert (nil? (get vals 2)) "hmget nil"))

                     (redis:hset "test:redis:hinc" "n" "10")
                     (assert (= (redis:hincrby "test:redis:hinc" "n" 5) 15)
                             "hincrby")

                     (println "  hash commands: ok")

                     # ── List commands ───────────────────────────────────────────────

                     (redis:rpush "test:redis:list" "a" "b" "c")
                     (assert (= (redis:llen "test:redis:list") 3) "llen")
                     (assert (= (redis:lindex "test:redis:list" 0) "a")
                             "lindex 0")
                     (assert (= (redis:lindex "test:redis:list" 2) "c")
                             "lindex 2")

                     (let [range (redis:lrange "test:redis:list" 0 -1)]
                       (assert (= (length range) 3) "lrange length")
                       (assert (= (get range 0) "a") "lrange 0")
                       (assert (= (get range 2) "c") "lrange 2"))

                     (redis:lpush "test:redis:list" "z")
                     (assert (= (redis:lpop "test:redis:list") "z") "lpop")
                     (assert (= (redis:rpop "test:redis:list") "c") "rpop")

                     (redis:lset "test:redis:list" 0 "A")
                     (assert (= (redis:lindex "test:redis:list" 0) "A") "lset")

                     (println "  list commands: ok")

                     # ── Set commands ────────────────────────────────────────────────

                     (redis:sadd "test:redis:set1" "a" "b" "c")
                     (assert (= (redis:scard "test:redis:set1") 3) "scard")
                     (assert (= (redis:sismember "test:redis:set1" "a") true)
                             "sismember true")
                     (assert (= (redis:sismember "test:redis:set1" "z") false)
                             "sismember false")

                     (redis:srem "test:redis:set1" "c")
                     (assert (= (redis:scard "test:redis:set1") 2)
                             "scard after srem")

                     (redis:sadd "test:redis:set2" "b" "c" "d")
                     (let [u (redis:sunion "test:redis:set1" "test:redis:set2")]
                       (assert (>= (length u) 3) "sunion"))
                     (let [i (redis:sinter "test:redis:set1" "test:redis:set2")]
                       (assert (>= (length i) 1) "sinter"))

                     (println "  set commands: ok")

                     # ── Sorted set commands ─────────────────────────────────────────

                     (redis:zadd "test:redis:zset" 1 "a")
                     (redis:zadd "test:redis:zset" 2 "b")
                     (redis:zadd "test:redis:zset" 3 "c")

                     (assert (= (redis:zcard "test:redis:zset") 3) "zcard")
                     (assert (= (redis:zscore "test:redis:zset" "b") "2")
                             "zscore")
                     (assert (= (redis:zrank "test:redis:zset" "a") 0) "zrank")

                     (let [range (redis:zrange "test:redis:zset" 0 -1)]
                       (assert (= (length range) 3) "zrange length")
                       (assert (= (get range 0) "a") "zrange 0"))

                     (redis:zrem "test:redis:zset" "c")
                     (assert (= (redis:zcard "test:redis:zset") 2)
                             "zcard after zrem")

                     (println "  sorted set commands: ok")

                     # ── Pipeline ────────────────────────────────────────────────────

                     (redis:set "test:redis:p1" "x")
                     (redis:set "test:redis:p2" "y")
                     (let [results (redis:pipeline (list "GET" "test:redis:p1")
                           (list "GET" "test:redis:p2") (list "PING"))]
                       (assert (= (get results 0) "x") "pipeline get 0")
                       (assert (= (get results 1) "y") "pipeline get 1")
                       (assert (= (get results 2) "PONG") "pipeline ping"))

                     (println "  pipeline: ok")

                     # ── DBSIZE ──────────────────────────────────────────────────────

                     (let [sz (redis:dbsize)]
                       (assert (> sz 0) "dbsize"))
                     (println "  dbsize: ok")

                     # ================================================================
                     # 2. Stress tests
                     # ================================================================

                     (clear-test-keys)

                     # 100 PINGs
                     (def @i 0)
                     (while (< i 100)
                       (assert (= (redis:ping) "PONG")
                               (concat "ping failed at " (string i)))
                       (assign i (+ i 1)))
                     (println "  100 pings: ok")

                     # 50 SET/GET pairs
                     (assign i 0)
                     (while (< i 50)
                       (let [key (concat "test:redis:sg:" (string i))
                             val (concat "value-" (string i))]
                         (assert (= (redis:set key val) true)
                                 (concat "set failed at " (string i)))
                         (assert (= (redis:get key) val)
                                 (concat "get failed at " (string i))))
                       (assign i (+ i 1)))
                     (println "  50 set/get pairs: ok")

                     # Mixed response types
                     (clear-test-keys)
                     (redis:set "test:redis:k1" "v1")
                     (redis:get "test:redis:k1")
                     (redis:get "test:redis:nonexistent")
                     (redis:set "test:redis:nx" "first" :nx true)
                     (redis:get "test:redis:nx")
                     (redis:set "test:redis:counter" "10")
                     (redis:incr "test:redis:counter")
                     (redis:decr "test:redis:counter")
                     (redis:incrby "test:redis:counter" 5)
                     (redis:decrby "test:redis:counter" 3)
                     (redis:set "test:redis:str" "hello")
                     (redis:append "test:redis:str" " world")
                     (redis:get "test:redis:str")
                     (redis:strlen "test:redis:str")
                     (redis:mset "test:redis:m1" "a" "test:redis:m2" "b")
                     (redis:mget "test:redis:m1" "test:redis:m2"
                                 "test:redis:nonexistent")
                     (redis:setnx "test:redis:setnx" "val")
                     (redis:setnx "test:redis:setnx" "val2")
                     (assert (= (redis:exists "test:redis:k1") true)
                             "stress exists true")
                     (assert (= (redis:exists "test:redis:nonexistent") false)
                             "stress exists false")
                     (println "  mixed commands: ok")

                     # ── ev/spawn inside redis-with ─────────────────────────────────
                     #
                     # Regression: redis:with binds *redis-port* via parameterize.
                     # A fiber spawned with ev/spawn inside the body is resumed
                     # later by the scheduler, whose own param_frames do not
                     # contain *redis-port*.  Only creation-time inheritance in
                     # fiber/new makes the child see the connection.  Without it
                     # the redis:get call fails with :no-connection.

                     # Mixed response types
                     (clear-test-keys)
                     (redis:set "test:redis:k1" "v1")
                     (redis:get "test:redis:k1")
                     (redis:get "test:redis:nonexistent")
                     (redis:set "test:redis:nx" "first" :nx true)
                     (redis:get "test:redis:nx")
                     (redis:set "test:redis:counter" "10")
                     (redis:incr "test:redis:counter")
                     (redis:decr "test:redis:counter")
                     (redis:incrby "test:redis:counter" 5)
                     (redis:decrby "test:redis:counter" 3)
                     (redis:set "test:redis:str" "hello")
                     (redis:append "test:redis:str" " world")
                     (redis:get "test:redis:str")
                     (redis:strlen "test:redis:str")
                     (redis:mset "test:redis:m1" "a" "test:redis:m2" "b")
                     (redis:mget "test:redis:m1" "test:redis:m2"
                                 "test:redis:nonexistent")
                     (redis:setnx "test:redis:setnx" "val")
                     (redis:setnx "test:redis:setnx" "val2")
                     (assert (= (redis:exists "test:redis:k1") true)
                             "stress exists true")
                     (assert (= (redis:exists "test:redis:nonexistent") false)
                             "stress exists false")
                     (println "  mixed commands: ok")

                     # ── ev/spawn inside redis-with ─────────────────────────────────
                     #
                     # Regression: redis:with binds *redis-port* via parameterize.
                     # A fiber spawned with ev/spawn inside the body is resumed
                     # later by the scheduler, whose own param_frames do not
                     # contain *redis-port*.  Only creation-time inheritance in
                     # fiber/new makes the child see the connection.  Without it
                     # the redis:get call fails with :no-connection.

                     (redis:set "test:redis:spawn-canary" "preset")
                     (let [result-box (box nil)
                           f (ev/spawn (fn []
                                         (rebox result-box
                                         (redis:get "test:redis:spawn-canary"))))]
                       (ev/join f)
                       (assert (= (unbox result-box) "preset")
                               "spawned fiber sees *redis-port* inherited from spawner"))
                     (println "  ev/spawn inside redis-with: ok")

                     # ── Cleanup ─────────────────────────────────────────────────────

                     (clear-test-keys)
                     (println "redis: all tests passed"))))
