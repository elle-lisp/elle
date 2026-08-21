(elle/epoch 12)
# tests/elle/redis.lisp — Redis integration tests
#
# Requires a live Redis on 127.0.0.1:6379.
# All tests run within a single connection; the test cleans up only its own
# keys via SCAN+DEL so it never touches db0 (or any db) as a whole.
#
# Each tests/elle/redis*.lisp file owns a distinct "test:<name>:*"
# sub-namespace, so the runner can fire DIFFERENT redis files concurrently
# against a shared Redis without one wiping another's keys mid-run.

(def redis ((import-file "lib/redis.lisp")))

# Redis is shared infrastructure, so this file's keyspace carries the running
# process's pid.  The per-file namespace separates the files; the pid separates
# the runs.  Without it a fixed name collides the way a fixed scratch filename
# does: a second run against the same server — another checkout, a rerun
# overlapping this one — writes the same keys, and each run's SCAN+DEL cleanup
# deletes the other's mid-flight.  The reader then sees its own value missing
# and fails an assertion that has nothing to do with Redis.
#
# Build every key with (test-key "name").  A bare key literal escapes the pid
# scope and reopens the collision.
(def key-prefix (string "test:redis:" (sys/pid) ":"))

(defn test-key [name]
  (string key-prefix name))

# Delete only keys created by this run, so we never run FLUSHDB / FLUSHALL
# against a shared Redis. Safe to call when no keys match.
# COUNT 1000: SCAN pages the whole keyspace and MATCH filters each page, so the
# server-default page size (10) costs one round trip per ~10 ambient keys. On a
# shared Redis with tens of thousands of keys that is thousands of round trips
# per call — this file calls it four times — enough to trip the smoke timeout.
(defn clear-test-keys []
  (let [keys (redis:scan-all redis:scan :match (string key-prefix "*")
                             :count 1000)]
    (when (not (empty? keys)) (apply redis:del keys))))

# Each section below runs inside the (redis:with …) body, so it reads
# *redis-port* from the enclosing parameterize.  They are top-level functions
# only to keep the connection block shallow; the runner treats the whole file
# as one thunk, so a definition is never scored as a test on its own.

# ── String and key commands ─────────────────────────────────────────────────

(defn string-commands []
  (assert (= (redis:set (test-key "k1") "v1") true) "set")
  (assert (= (redis:get (test-key "k1")) "v1") "get")
  (assert (nil? (redis:get (test-key "nonexistent"))) "get nil")

  (assert (= (redis:set (test-key "nx") "first" :nx true) true) "set nx first")
  (assert (= (redis:get (test-key "nx")) "first") "get nx first")

  (redis:set (test-key "counter") "10")
  (assert (= (redis:incr (test-key "counter")) 11) "incr")
  (assert (= (redis:decr (test-key "counter")) 10) "decr")
  (assert (= (redis:incrby (test-key "counter") 5) 15) "incrby")
  (assert (= (redis:decrby (test-key "counter") 3) 12) "decrby")

  (redis:set (test-key "str") "hello")
  (redis:append (test-key "str") " world")
  (assert (= (redis:get (test-key "str")) "hello world") "append")
  (assert (= (redis:strlen (test-key "str")) 11) "strlen")

  (redis:mset (test-key "m1") "a" (test-key "m2") "b")
  (let [vals (redis:mget (test-key "m1") (test-key "m2")
                         (test-key "nonexistent"))]
    (assert (= (get vals 0) "a") "mget 0")
    (assert (= (get vals 1) "b") "mget 1")
    (assert (nil? (get vals 2)) "mget nil"))

  (assert (= (redis:setnx (test-key "setnx") "val") true) "setnx new")
  (assert (= (redis:setnx (test-key "setnx") "val2") false) "setnx exists")

  (assert (= (redis:exists (test-key "k1")) true) "exists true")
  (assert (= (redis:exists (test-key "nonexistent")) false) "exists false")

  (redis:set (test-key "exp") "val")
  (assert (= (redis:expire (test-key "exp") 100) true) "expire")
  (let [ttl (redis:ttl (test-key "exp"))]
    (assert (> ttl 0) "ttl positive"))
  (assert (= (redis:persist (test-key "exp")) true) "persist")
  (assert (= (redis:ttl (test-key "exp")) -1) "ttl after persist")

  (redis:set (test-key "rename") "val")
  (assert (= (redis:rename (test-key "rename") (test-key "renamed")) true)
          "rename")
  (assert (= (redis:get (test-key "renamed")) "val") "get renamed")

  (assert (>= (redis:del (test-key "k1") (test-key "renamed")) 1) "del")
  (assert (= (redis:exists (test-key "k1")) false) "exists after del"))

# ── Hash commands ───────────────────────────────────────────────────────────

(defn hash-commands []
  (redis:hset (test-key "hash") "name" "Alice")
  (redis:hset (test-key "hash") "age" "30")

  (assert (= (redis:hget (test-key "hash") "name") "Alice") "hget")
  (assert (nil? (redis:hget (test-key "hash") "missing")) "hget nil")
  (assert (= (redis:hexists (test-key "hash") "name") true) "hexists true")
  (assert (= (redis:hexists (test-key "hash") "missing") false) "hexists false")

  (let [h (redis:hgetall (test-key "hash"))]
    (assert (= (get h "name") "Alice") "hgetall name")
    (assert (= (get h "age") "30") "hgetall age"))

  (redis:hmset (test-key "hm") "a" "1" "b" "2" "c" "3")
  (let [vals (redis:hmget (test-key "hm") "a" "c" "missing")]
    (assert (= (get vals 0) "1") "hmget 0")
    (assert (= (get vals 1) "3") "hmget 1")
    (assert (nil? (get vals 2)) "hmget nil"))

  (redis:hset (test-key "hinc") "n" "10")
  (assert (= (redis:hincrby (test-key "hinc") "n" 5) 15) "hincrby")

  (println "  hash commands: ok"))

# ── List commands ───────────────────────────────────────────────────────────

(defn list-commands []
  (redis:rpush (test-key "list") "a" "b" "c")
  (assert (= (redis:llen (test-key "list")) 3) "llen")
  (assert (= (redis:lindex (test-key "list") 0) "a") "lindex 0")
  (assert (= (redis:lindex (test-key "list") 2) "c") "lindex 2")

  (let [range (redis:lrange (test-key "list") 0 -1)]
    (assert (= (length range) 3) "lrange length")
    (assert (= (get range 0) "a") "lrange 0")
    (assert (= (get range 2) "c") "lrange 2"))

  (redis:lpush (test-key "list") "z")
  (assert (= (redis:lpop (test-key "list")) "z") "lpop")
  (assert (= (redis:rpop (test-key "list")) "c") "rpop")

  (redis:lset (test-key "list") 0 "A")
  (assert (= (redis:lindex (test-key "list") 0) "A") "lset")

  (println "  list commands: ok"))

# ── Set commands ────────────────────────────────────────────────────────────

(defn set-commands []
  (redis:sadd (test-key "set1") "a" "b" "c")
  (assert (= (redis:scard (test-key "set1")) 3) "scard")
  (assert (= (redis:sismember (test-key "set1") "a") true) "sismember true")
  (assert (= (redis:sismember (test-key "set1") "z") false) "sismember false")

  (redis:srem (test-key "set1") "c")
  (assert (= (redis:scard (test-key "set1")) 2) "scard after srem")

  (redis:sadd (test-key "set2") "b" "c" "d")
  (let [u (redis:sunion (test-key "set1") (test-key "set2"))]
    (assert (>= (length u) 3) "sunion"))
  (let [i (redis:sinter (test-key "set1") (test-key "set2"))]
    (assert (>= (length i) 1) "sinter"))

  (println "  set commands: ok"))

# ── Sorted set commands ─────────────────────────────────────────────────────

(defn zset-commands []
  (redis:zadd (test-key "zset") 1 "a")
  (redis:zadd (test-key "zset") 2 "b")
  (redis:zadd (test-key "zset") 3 "c")

  (assert (= (redis:zcard (test-key "zset")) 3) "zcard")
  (assert (= (redis:zscore (test-key "zset") "b") "2") "zscore")
  (assert (= (redis:zrank (test-key "zset") "a") 0) "zrank")

  (let [range (redis:zrange (test-key "zset") 0 -1)]
    (assert (= (length range) 3) "zrange length")
    (assert (= (get range 0) "a") "zrange 0"))

  (redis:zrem (test-key "zset") "c")
  (assert (= (redis:zcard (test-key "zset")) 2) "zcard after zrem")

  (println "  sorted set commands: ok"))

# ── Pipeline and DBSIZE ─────────────────────────────────────────────────────

(defn pipeline-commands []
  (redis:set (test-key "p1") "x")
  (redis:set (test-key "p2") "y")
  (let [get-p1 (list "GET" (test-key "p1"))
        get-p2 (list "GET" (test-key "p2"))
        results (redis:pipeline get-p1 get-p2 (list "PING"))]
    (assert (= (get results 0) "x") "pipeline get 0")
    (assert (= (get results 1) "y") "pipeline get 1")
    (assert (= (get results 2) "PONG") "pipeline ping"))
  (println "  pipeline: ok")

  (let [sz (redis:dbsize)]
    (assert (> sz 0) "dbsize"))
  (println "  dbsize: ok"))

# ── Stress ──────────────────────────────────────────────────────────────────

(defn stress-tests []
  (clear-test-keys)

  (def @i 0)
  (while (< i 100)
    (assert (= (redis:ping) "PONG") (string "ping failed at " i))
    (assign i (+ i 1)))
  (println "  100 pings: ok")

  (assign i 0)
  (while (< i 50)
    (let [key (test-key (string "sg:" i))
          val (string "value-" i)]
      (assert (= (redis:set key val) true) (string "set failed at " i))
      (assert (= (redis:get key) val) (string "get failed at " i)))
    (assign i (+ i 1)))
  (println "  50 set/get pairs: ok")

  # Mixed response types: every reply shape the RESP parser handles, back to
  # back, so a framing slip in one shape shows up in the next assertion.
  (clear-test-keys)
  (redis:set (test-key "k1") "v1")
  (redis:get (test-key "k1"))
  (redis:get (test-key "nonexistent"))
  (redis:set (test-key "nx") "first" :nx true)
  (redis:get (test-key "nx"))
  (redis:set (test-key "counter") "10")
  (redis:incr (test-key "counter"))
  (redis:decr (test-key "counter"))
  (redis:incrby (test-key "counter") 5)
  (redis:decrby (test-key "counter") 3)
  (redis:set (test-key "str") "hello")
  (redis:append (test-key "str") " world")
  (redis:get (test-key "str"))
  (redis:strlen (test-key "str"))
  (redis:mset (test-key "m1") "a" (test-key "m2") "b")
  (redis:mget (test-key "m1") (test-key "m2") (test-key "nonexistent"))
  (redis:setnx (test-key "setnx") "val")
  (redis:setnx (test-key "setnx") "val2")
  (assert (= (redis:exists (test-key "k1")) true) "stress exists true")
  (assert (= (redis:exists (test-key "nonexistent")) false)
          "stress exists false")
  (println "  mixed commands: ok"))

# ── ev/spawn inside redis-with ──────────────────────────────────────────────
#
# redis:with binds *redis-port* via parameterize.  A fiber spawned with
# ev/spawn inside the body is resumed later by the scheduler, whose own
# param_frames do not contain *redis-port*.  Creation-time inheritance in
# fiber/new is what makes the child see the connection; without it the
# redis:get call fails with :no-connection.

(defn spawn-inheritance []
  (redis:set (test-key "spawn-canary") "preset")
  (let [result-box (box nil)
        f (ev/spawn (fn []
                      (rebox result-box (redis:get (test-key "spawn-canary")))))]
    (ev/join f)
    (assert (= (unbox result-box) "preset")
            "spawned fiber sees *redis-port* inherited from spawner"))
  (println "  ev/spawn inside redis-with: ok"))

# ── RESP self-tests (no Redis needed) ───────────────────────────────────────

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
                     (assert (= (redis:ping) "PONG") "ping")
                     (println "  ping: ok")
                     (assert (= (redis:echo "hello") "hello") "echo")
                     (println "  echo: ok")

                     (clear-test-keys)
                     (string-commands)
                     (hash-commands)
                     (list-commands)
                     (set-commands)
                     (zset-commands)
                     (pipeline-commands)
                     (stress-tests)
                     (spawn-inheritance)

                     (clear-test-keys)
                     (println "redis: all tests passed"))))
