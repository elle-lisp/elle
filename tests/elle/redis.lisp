# Redis integration tests — requires a running Redis on localhost:6379
# Run: elle tests/elle/redis.lisp

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false} ((import-file "tests/elle/assert.lisp")))
(def redis ((import-file "lib/redis.lisp")))

(ev/run (fn [] (let [[c (redis:connect "127.0.0.1" 6379)]] (redis:flushdb c) (assert-eq (redis:ping c) "PONG" "ping") (assert-true (redis:set c "k" "v") "set") (redis:close c))))
(ev/run (fn [] (let [[c (redis:connect "127.0.0.1" 6379)]] (assert-eq (redis:get c "k") "v" "get") (assert-eq (redis:del c "k") 1 "del") (redis:close c))))
(ev/run (fn [] (let [[c (redis:connect "127.0.0.1" 6379)]] (redis:set c "n" "10") (assert-eq (redis:incr c "n") 11 "incr") (redis:close c))))
(ev/run (fn [] (let [[c (redis:connect "127.0.0.1" 6379)]] (assert-true (redis:set c "nx" "v" :nx true) "nx") (redis:lpush c "l" "a") (redis:close c))))
(ev/run (fn [] (let [[c (redis:connect "127.0.0.1" 6379)]] (assert-eq (redis:lpop c "l") "a" "lpop") (redis:hset c "h" "k" "v") (redis:close c))))
(ev/run (fn [] (let [[c (redis:connect "127.0.0.1" 6379)]] (assert-eq (redis:hget c "h" "k") "v" "hget") (redis:sadd c "s" "a") (redis:close c))))
(ev/run (fn [] (let [[c (redis:connect "127.0.0.1" 6379)]] (assert-true (redis:sismember c "s" "a") "sismember") (redis:zadd c "z" 1 "x") (redis:close c))))
(ev/run (fn [] (let [[c (redis:connect "127.0.0.1" 6379)]] (assert-eq (redis:zrank c "z" "x") 0 "zrank") (assert-eq (redis:command c ["ECHO" "hi"]) "hi" "raw") (redis:close c))))
(print "all redis tests passed")
