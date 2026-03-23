## tests/elle/redis-stress.lisp — Redis command type stress tests

(def redis ((import-file "lib/redis.lisp")))

# Skip if Redis is not reachable
(let [[[ok? _] (protect (tcp/connect "127.0.0.1" 6379))]]
  (when (not ok?)
    (println "SKIP: Redis not available at 127.0.0.1:6379")
    (exit 0)))

(redis:with "127.0.0.1" 6379
  (fn []
    (redis:flushdb)

    # Test 1: 100 PINGs (simple string only)
    (var i 0)
    (while (< i 100)
      (assert (= (redis:ping) "PONG")
        (string/join ["ping failed at " (string i)] ""))
      (assign i (+ i 1)))
    (println "  100 pings: ok")

    # Test 2: 50 SET/GET pairs (bulk string responses)
    (assign i 0)
    (while (< i 50)
      (let [[key (string/join ["test:sg:" (string i)] "")]
            [val (string/join ["value-" (string i)] "")]]
        (assert (= (redis:set key val) true)
          (string/join ["set failed at " (string i)] ""))
        (assert (= (redis:get key) val)
          (string/join ["get failed at " (string i)] "")))
      (assign i (+ i 1)))
    (println "  50 set/get pairs: ok")

    # Test 3: mixed response types (the failing sequence)
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
    (assert (= (redis:exists "test:k1") true) "exists true")
    (assert (= (redis:exists "test:nonexistent") false) "exists false")
    (println "  mixed commands: ok")

    (println "redis-stress: all tests passed")))
