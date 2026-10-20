(elle/epoch 15)
# audited: 2026-10-20
## Hash, list, set and sorted-set commands, each one redis-cmd with its reply converted.
## lib/redis/overview.md
##
## Loaded via:
##   (def collections ((import "std/redis/collections") :connection connection :resp resp))
##
## Exports: {:hset :hget :hdel :hexists :hgetall :hkeys :hvals :hlen :hmset
##           :hmget :hincrby :lpush :rpush :lpop :rpop :llen :lrange :lindex
##           :lset :sadd :srem :sismember :smembers :scard :sunion :sinter
##           :sdiff :zadd :zscore :zrank :zrange :zrangebyscore :zrem :zcard
##           :zincrby :zcount :zrevrange :zrevrangebyscore :zrange-withscores
##           :zrangebyscore-withscores :zrevrange-withscores}

(fn [&named connection resp]
  (def redis-cmd connection:cmd)
  (def resp-ok? resp:ok?)
  (def resp-bool resp:bool)

  ## ── Commands — Hash ───────────────────────────────────────────────────

  (defn redis-hset [key field value]
    "HSET key field value — returns 1 if new field, 0 if updated."
    (redis-cmd "HSET" key field value))

  (defn redis-hget [key field]
    "HGET key field — returns value or nil."
    (redis-cmd "HGET" key field))

  (defn redis-hdel [key & fields]
    "HDEL key field [field ...] — returns count of deleted."
    (apply redis-cmd (pair "HDEL" (pair key fields))))

  (defn redis-hexists [key field]
    "HEXISTS key field — returns true if field exists."
    (resp-bool (redis-cmd "HEXISTS" key field)))

  (defn redis-hgetall [key]
    "HGETALL key — returns struct with string keys."
    (let [arr (redis-cmd "HGETALL" key)
          result @{}]
      (def @i 0)
      (while (< i (length arr))
        (put result (get arr i) (get arr (+ i 1)))
        (assign i (+ i 2)))
      (freeze result)))

  (defn redis-hkeys [key]
    "HKEYS key — returns array of field names."
    (redis-cmd "HKEYS" key))

  (defn redis-hvals [key]
    "HVALS key — returns array of values."
    (redis-cmd "HVALS" key))

  (defn redis-hlen [key]
    "HLEN key — returns number of fields."
    (redis-cmd "HLEN" key))

  (defn redis-hmset [key & pairs]
    "HMSET key field value [field value ...] — returns true on OK."
    (resp-ok? (apply redis-cmd (pair "HMSET" (pair key pairs)))))

  (defn redis-hmget [key & fields]
    "HMGET key field [field ...] — returns array of values."
    (apply redis-cmd (pair "HMGET" (pair key fields))))

  (defn redis-hincrby [key field n]
    "HINCRBY key field increment — returns new integer value."
    (redis-cmd "HINCRBY" key field (string n)))

  ## ── Commands — List ───────────────────────────────────────────────────

  (defn redis-lpush [key & values]
    "LPUSH key value [value ...] — returns new length."
    (apply redis-cmd (pair "LPUSH" (pair key values))))

  (defn redis-rpush [key & values]
    "RPUSH key value [value ...] — returns new length."
    (apply redis-cmd (pair "RPUSH" (pair key values))))

  (defn redis-lpop [key]
    "LPOP key — returns element or nil."
    (redis-cmd "LPOP" key))

  (defn redis-rpop [key]
    "RPOP key — returns element or nil."
    (redis-cmd "RPOP" key))

  (defn redis-llen [key]
    "LLEN key — returns length."
    (redis-cmd "LLEN" key))

  (defn redis-lrange [key start stop]
    "LRANGE key start stop — returns array of elements."
    (redis-cmd "LRANGE" key (string start) (string stop)))

  (defn redis-lindex [key index]
    "LINDEX key index — returns element or nil."
    (redis-cmd "LINDEX" key (string index)))

  (defn redis-lset [key index value]
    "LSET key index value — returns true on OK."
    (resp-ok? (redis-cmd "LSET" key (string index) value)))

  ## ── Commands — Set ────────────────────────────────────────────────────

  (defn redis-sadd [key & members]
    "SADD key member [member ...] — returns count of new members."
    (apply redis-cmd (pair "SADD" (pair key members))))

  (defn redis-srem [key & members]
    "SREM key member [member ...] — returns count of removed."
    (apply redis-cmd (pair "SREM" (pair key members))))

  (defn redis-sismember [key member]
    "SISMEMBER key member — returns true if member."
    (resp-bool (redis-cmd "SISMEMBER" key member)))

  (defn redis-smembers [key]
    "SMEMBERS key — returns array of members."
    (redis-cmd "SMEMBERS" key))

  (defn redis-scard [key]
    "SCARD key — returns count of members."
    (redis-cmd "SCARD" key))

  (defn redis-sunion [& keys]
    "SUNION key [key ...] — returns array of union members."
    (apply redis-cmd (pair "SUNION" keys)))

  (defn redis-sinter [& keys]
    "SINTER key [key ...] — returns array of intersection members."
    (apply redis-cmd (pair "SINTER" keys)))

  (defn redis-sdiff [& keys]
    "SDIFF key [key ...] — returns array of difference members."
    (apply redis-cmd (pair "SDIFF" keys)))

  ## ── Commands — Sorted Set ─────────────────────────────────────────────

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

  {:hset redis-hset
   :hget redis-hget
   :hdel redis-hdel
   :hexists redis-hexists
   :hgetall redis-hgetall
   :hkeys redis-hkeys
   :hvals redis-hvals
   :hlen redis-hlen
   :hmset redis-hmset
   :hmget redis-hmget
   :hincrby redis-hincrby
   :lpush redis-lpush
   :rpush redis-rpush
   :lpop redis-lpop
   :rpop redis-rpop
   :llen redis-llen
   :lrange redis-lrange
   :lindex redis-lindex
   :lset redis-lset
   :sadd redis-sadd
   :srem redis-srem
   :sismember redis-sismember
   :smembers redis-smembers
   :scard redis-scard
   :sunion redis-sunion
   :sinter redis-sinter
   :sdiff redis-sdiff
   :zadd redis-zadd
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
