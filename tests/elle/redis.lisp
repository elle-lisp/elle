(elle/epoch 9)
# tests/elle/redis.lisp — Redis integration tests
#
# Requires a live Redis on 127.0.0.1:6379.
# All tests run within a single connection to avoid parallel flushdb races.

(def redis ((import-file "lib/redis.lisp")))

# RESP self-tests (no Redis needed)
(println "Running RESP self-tests...")
(redis:test)
(println "RESP self-tests passed.")

# Skip if Redis is not reachable
(let [[ok? _] (protect (tcp/connect "127.0.0.1" 6379))]
  (when (not ok?)
    (println "SKIP: Redis not available at 127.0.0.1:6379")
    (exit 0)))

(println "Connecting to Redis at 127.0.0.1:6379...")

(redis:with "127.0.0.1" 6379
  (fn []

    # ================================================================
    # 1. Integration tests
    # ================================================================

    (assert (= (redis:ping) "PONG") "ping")
    (println "  ping: ok")

    (assert (= (redis:echo "hello") "hello") "echo")
    (println "  echo: ok")

    (redis:flushdb)

    # ── String commands ─────────────────────────────────────────────

    (assert (= (redis:set "test:k1" "v1") true) "set")
    (assert (= (redis:get "test:k1") "v1") "get")
    (assert (nil? (redis:get "test:nonexistent")) "get nil")

    (assert (= (redis:set "test:nx" "first" :nx true) true) "set nx first")
    (assert (= (redis:get "test:nx") "first") "get nx first")

    (redis:set "test:counter" "10")
    (assert (= (redis:incr "test:counter") 11) "incr")
    (assert (= (redis:decr "test:counter") 10) "decr")
    (assert (= (redis:incrby "test:counter" 5) 15) "incrby")
    (assert (= (redis:decrby "test:counter" 3) 12) "decrby")

    (redis:set "test:str" "hello")
    (redis:append "test:str" " world")
    (assert (= (redis:get "test:str") "hello world") "append")
    (assert (= (redis:strlen "test:str") 11) "strlen")

    (redis:mset "test:m1" "a" "test:m2" "b")
    (let [vals (redis:mget "test:m1" "test:m2" "test:nonexistent")]
      (assert (= (get vals 0) "a") "mget 0")
      (assert (= (get vals 1) "b") "mget 1")
      (assert (nil? (get vals 2)) "mget nil"))

    (assert (= (redis:setnx "test:setnx" "val") true) "setnx new")
    (assert (= (redis:setnx "test:setnx" "val2") false) "setnx exists")

    (println "  string commands: ok")

    # ── Key commands ────────────────────────────────────────────────

    (assert (= (redis:exists "test:k1") true) "exists true")
    (assert (= (redis:exists "test:nonexistent") false) "exists false")

    (redis:set "test:exp" "val")
    (assert (= (redis:expire "test:exp" 100) true) "expire")
    (let [ttl (redis:ttl "test:exp")]
      (assert (> ttl 0) "ttl positive"))
    (assert (= (redis:persist "test:exp") true) "persist")
    (assert (= (redis:ttl "test:exp") -1) "ttl after persist")

    (assert (= (redis:type "test:k1") "string") "type")

    (redis:set "test:rename" "val")
    (assert (= (redis:rename "test:rename" "test:renamed") true) "rename")
    (assert (= (redis:get "test:renamed") "val") "get renamed")

    (assert (>= (redis:del "test:k1" "test:renamed") 1) "del")
    (assert (= (redis:exists "test:k1") false) "exists after del")

    (println "  key commands: ok")

    # ── Hash commands ───────────────────────────────────────────────

    (redis:hset "test:hash" "name" "Alice")
    (redis:hset "test:hash" "age" "30")

    (assert (= (redis:hget "test:hash" "name") "Alice") "hget")
    (assert (nil? (redis:hget "test:hash" "missing")) "hget nil")
    (assert (= (redis:hexists "test:hash" "name") true) "hexists true")
    (assert (= (redis:hexists "test:hash" "missing") false) "hexists false")

    (let [h (redis:hgetall "test:hash")]
      (assert (= (get h "name") "Alice") "hgetall name")
      (assert (= (get h "age") "30") "hgetall age"))

    (assert (= (redis:hlen "test:hash") 2) "hlen")
    (redis:hdel "test:hash" "age")
    (assert (= (redis:hlen "test:hash") 1) "hlen after hdel")

    (redis:hmset "test:hm" "a" "1" "b" "2" "c" "3")
    (let [vals (redis:hmget "test:hm" "a" "c" "missing")]
      (assert (= (get vals 0) "1") "hmget 0")
      (assert (= (get vals 1) "3") "hmget 1")
      (assert (nil? (get vals 2)) "hmget nil"))

    (redis:hset "test:hinc" "n" "10")
    (assert (= (redis:hincrby "test:hinc" "n" 5) 15) "hincrby")

    (println "  hash commands: ok")

    # ── List commands ───────────────────────────────────────────────

    (redis:rpush "test:list" "a" "b" "c")
    (assert (= (redis:llen "test:list") 3) "llen")
    (assert (= (redis:lindex "test:list" 0) "a") "lindex 0")
    (assert (= (redis:lindex "test:list" 2) "c") "lindex 2")

    (let [range (redis:lrange "test:list" 0 -1)]
      (assert (= (length range) 3) "lrange length")
      (assert (= (get range 0) "a") "lrange 0")
      (assert (= (get range 2) "c") "lrange 2"))

    (redis:lpush "test:list" "z")
    (assert (= (redis:lpop "test:list") "z") "lpop")
    (assert (= (redis:rpop "test:list") "c") "rpop")

    (redis:lset "test:list" 0 "A")
    (assert (= (redis:lindex "test:list" 0) "A") "lset")

    (println "  list commands: ok")

    # ── Set commands ────────────────────────────────────────────────

    (redis:sadd "test:set1" "a" "b" "c")
    (assert (= (redis:scard "test:set1") 3) "scard")
    (assert (= (redis:sismember "test:set1" "a") true) "sismember true")
    (assert (= (redis:sismember "test:set1" "z") false) "sismember false")

    (redis:srem "test:set1" "c")
    (assert (= (redis:scard "test:set1") 2) "scard after srem")

    (redis:sadd "test:set2" "b" "c" "d")
    (let [u (redis:sunion "test:set1" "test:set2")]
      (assert (>= (length u) 3) "sunion"))
    (let [i (redis:sinter "test:set1" "test:set2")]
      (assert (>= (length i) 1) "sinter"))

    (println "  set commands: ok")

    # ── Sorted set commands ─────────────────────────────────────────

    (redis:zadd "test:zset" 1 "a")
    (redis:zadd "test:zset" 2 "b")
    (redis:zadd "test:zset" 3 "c")

    (assert (= (redis:zcard "test:zset") 3) "zcard")
    (assert (= (redis:zscore "test:zset" "b") "2") "zscore")
    (assert (= (redis:zrank "test:zset" "a") 0) "zrank")

    (let [range (redis:zrange "test:zset" 0 -1)]
      (assert (= (length range) 3) "zrange length")
      (assert (= (get range 0) "a") "zrange 0"))

    (redis:zrem "test:zset" "c")
    (assert (= (redis:zcard "test:zset") 2) "zcard after zrem")

    (println "  sorted set commands: ok")

    # ── Pipeline ────────────────────────────────────────────────────

    (redis:set "test:p1" "x")
    (redis:set "test:p2" "y")
    (let [results (redis:pipeline
                     (list "GET" "test:p1")
                     (list "GET" "test:p2")
                     (list "PING"))]
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

    (redis:flushdb)

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
      (let [key (concat "test:sg:" (string i))
            val (concat "value-" (string i))]
        (assert (= (redis:set key val) true)
          (concat "set failed at " (string i)))
        (assert (= (redis:get key) val)
          (concat "get failed at " (string i))))
      (assign i (+ i 1)))
    (println "  50 set/get pairs: ok")

    # Mixed response types
    (redis:flushdb)
    (redis:set "test:k1" "v1")
    (redis:get "test:k1")
    (redis:get "test:nonexistent")
    (redis:set "test:nx" "first" :nx true)
    (redis:get "test:nx")
    (redis:set "test:counter" "10")
    (redis:incr "test:counter")
    (redis:decr "test:counter")
    (redis:incrby "test:counter" 5)
    (redis:decrby "test:counter" 3)
    (redis:set "test:str" "hello")
    (redis:append "test:str" " world")
    (redis:get "test:str")
    (redis:strlen "test:str")
    (redis:mset "test:m1" "a" "test:m2" "b")
    (redis:mget "test:m1" "test:m2" "test:nonexistent")
    (redis:setnx "test:setnx" "val")
    (redis:setnx "test:setnx" "val2")
    (assert (= (redis:exists "test:k1") true) "stress exists true")
    (assert (= (redis:exists "test:nonexistent") false) "stress exists false")
    (println "  mixed commands: ok")

    # ── Cleanup ─────────────────────────────────────────────────────

    (redis:flushdb)
    (println "redis: all tests passed")))
