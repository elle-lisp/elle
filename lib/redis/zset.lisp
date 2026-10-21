(elle/epoch 15)
# audited: 2026-10-20
## Sorted-set commands, each one redis-cmd with its reply as Redis sends it.
## lib/redis/overview.md
##
## Loaded via:
##   (def zset ((import "std/redis/zset") :connection connection))
##
## Exports: {:zadd :zscore :zrank :zrange :zrangebyscore :zrem :zcard
##           :zincrby :zcount :zrevrange :zrevrangebyscore :zrange-withscores
##           :zrangebyscore-withscores :zrevrange-withscores}

(fn [&named connection]
  (def redis-cmd connection:cmd)

  (defn redis-zadd [key score member]
    "ZADD key score member — returns count of new members."
    (redis-cmd "ZADD" key (string score) member))

  (defn redis-zscore [key member]
    "ZSCORE key member — returns score string or nil."
    (redis-cmd "ZSCORE" key member))

  (defn redis-zrank [key member]
    "ZRANK key member — returns rank integer or nil."
    (redis-cmd "ZRANK" key member))

  (defn redis-zrange [key start stop]
    "ZRANGE key start stop — returns array of members."
    (redis-cmd "ZRANGE" key (string start) (string stop)))

  (defn redis-zrangebyscore [key min max]
    "ZRANGEBYSCORE key min max — returns array of members."
    (redis-cmd "ZRANGEBYSCORE" key (string min) (string max)))

  (defn redis-zrem [key & members]
    "ZREM key member [member ...] — returns count of removed."
    (apply redis-cmd (pair "ZREM" (pair key members))))

  (defn redis-zcard [key]
    "ZCARD key — returns count of members."
    (redis-cmd "ZCARD" key))

  (defn redis-zincrby [key increment member]
    "ZINCRBY key increment member — increment score, returns new score string."
    (redis-cmd "ZINCRBY" key (string increment) member))

  (defn redis-zcount [key min max]
    "ZCOUNT key min max — count members with score in [min, max]."
    (redis-cmd "ZCOUNT" key (string min) (string max)))

  (defn redis-zrevrange [key start stop]
    "ZREVRANGE key start stop — members in descending score order."
    (redis-cmd "ZREVRANGE" key (string start) (string stop)))

  (defn redis-zrevrangebyscore [key max min]
    "ZREVRANGEBYSCORE key max min — members by score, descending."
    (redis-cmd "ZREVRANGEBYSCORE" key (string max) (string min)))

  (defn redis-zrange-withscores [key start stop]
    "ZRANGE key start stop WITHSCORES — returns flat array [member score member score ...]."
    (redis-cmd "ZRANGE" key (string start) (string stop) "WITHSCORES"))

  (defn redis-zrangebyscore-withscores [key min max]
    "ZRANGEBYSCORE key min max WITHSCORES — returns flat array [member score ...]."
    (redis-cmd "ZRANGEBYSCORE" key (string min) (string max) "WITHSCORES"))

  (defn redis-zrevrange-withscores [key start stop]
    "ZREVRANGE key start stop WITHSCORES — returns flat array [member score ...]."
    (redis-cmd "ZREVRANGE" key (string start) (string stop) "WITHSCORES"))

  {:zadd redis-zadd
   :zscore redis-zscore
   :zrank redis-zrank
   :zrange redis-zrange
   :zrangebyscore redis-zrangebyscore
   :zrem redis-zrem
   :zcard redis-zcard
   :zincrby redis-zincrby
   :zcount redis-zcount
   :zrevrange redis-zrevrange
   :zrevrangebyscore redis-zrevrangebyscore
   :zrange-withscores redis-zrange-withscores
   :zrangebyscore-withscores redis-zrangebyscore-withscores
   :zrevrange-withscores redis-zrevrange-withscores})
