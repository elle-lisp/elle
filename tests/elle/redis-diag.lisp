## Diagnostic: trace redis commands to find where it silently fails

(def redis ((import-file "lib/redis.lisp")))

# Skip if Redis is not reachable
(let [[[ok? _] (protect (tcp/connect "127.0.0.1" 6379))]]
  (when (not ok?)
    (println "SKIP: Redis not available at 127.0.0.1:6379")
    (exit 0)))

(let [[[ok? val] (protect
  (redis:with "127.0.0.1" 6379
    (fn []
      (redis:flushdb)

      # Test: 50 SET/GET pairs
      (var i 0)
      (while (< i 50)
        (let [[key (string "k:" i)]
              [val (string "v" i)]]
          (redis:set key val)
          (let [[g (redis:get key)]]
            (when (not (= g val))
              (println (string "MISMATCH at " i ": expected " val " got " g)))))
        (assign i (+ i 1)))
      (println "  50 set/get pairs: ok")

      # Test: mixed commands
      (redis:flushdb)
      (redis:set "test:k1" "v1")
      (println "  after set k1")
      (redis:get "test:k1")
      (println "  after get k1")
      (redis:get "test:nonexistent")
      (println "  after get nonexistent")
      (redis:set "test:nx" "first" :nx true)
      (println "  after set nx")
      (redis:get "test:nx")
      (println "  after get nx")
      (redis:set "test:counter" "10")
      (println "  after set counter")
      (redis:incr "test:counter")
      (println "  after incr")
      (redis:decr "test:counter")
      (println "  after decr")
      (redis:incrby "test:counter" 5)
      (println "  after incrby")
      (redis:decrby "test:counter" 3)
      (println "  after decrby")
      (redis:set "test:str" "hello")
      (println "  after set str")
      (redis:append "test:str" " world")
      (println "  after append")
      (redis:get "test:str")
      (println "  after get str")
      (redis:strlen "test:str")
      (println "  after strlen")
      (redis:mset "test:m1" "a" "test:m2" "b")
      (println "  after mset")
      (redis:mget "test:m1" "test:m2" "test:nonexistent")
      (println "  after mget")
      (redis:setnx "test:setnx" "val")
      (println "  after setnx 1")
      (redis:setnx "test:setnx" "val2")
      (println "  after setnx 2")
      (println "  about to call EXISTS...")
      (redis:exists "test:k1")
      (println "  after exists k1")
      (redis:exists "test:nonexistent")
      (println "  after exists nonexistent")
      (println "  mixed: ok")
      (println "all done"))))]]
  (if ok?
    (println "SUCCESS")
    (println (string "ERROR: " val))))
