(elle/epoch 15)
# audited: 2026-10-20
## Redis client speaking RESP2 over TCP, in pure Elle with no plugin behind it.
## lib/redis.md
## lib/redis/overview.md
##
## Loaded via: (def redis ((import "std/redis")))
## Usage:      (redis:set "key" "value")
##             (redis:get "key")
##
## Connection model: {:port :framer}, the TCP port written and the
## std/framing framer that answers the replies (lib/redis.md).
## Error model: manager fiber owns the connection, reconnects on transient
## errors.
## Protocol: RESP2 over TCP.

## ── Submodules ────────────────────────────────────────────────────────

(def resp ((import "std/redis/resp")))
(def connection ((import "std/redis/connection") :resp resp))
(def keyspace ((import "std/redis/keyspace") :connection connection :resp resp))
(def collections
  ((import "std/redis/collections") :connection connection :resp resp))
(def zset ((import "std/redis/zset") :connection connection))
(def server ((import "std/redis/server") :connection connection :resp resp))

## ── Exports ───────────────────────────────────────────────────────────

(fn []
  {# Connection
   :connection resp:connection
   :connect resp:connect
   :close resp:close
   :with connection:with
   :auth connection:auth

   # Manager
   :manager connection:manager

   # Commands — String
   :get keyspace:get
   :set keyspace:set
   :mget keyspace:mget
   :mset keyspace:mset
   :incr keyspace:incr
   :decr keyspace:decr
   :incrby keyspace:incrby
   :decrby keyspace:decrby
   :append keyspace:append
   :strlen keyspace:strlen
   :getset keyspace:getset
   :setnx keyspace:setnx

   # Commands — Keys
   :del keyspace:del
   :exists keyspace:exists
   :expire keyspace:expire
   :pexpire keyspace:pexpire
   :expireat keyspace:expireat
   :pexpireat keyspace:pexpireat
   :ttl keyspace:ttl
   :pttl keyspace:pttl
   :type keyspace:type
   :keys keyspace:keys
   :rename keyspace:rename
   :persist keyspace:persist

   # Commands — Scan
   :scan keyspace:scan
   :hscan keyspace:hscan
   :sscan keyspace:sscan
   :zscan keyspace:zscan
   :scan-all keyspace:scan-all

   # Commands — Hash
   :hset collections:hset
   :hget collections:hget
   :hdel collections:hdel
   :hexists collections:hexists
   :hgetall collections:hgetall
   :hkeys collections:hkeys
   :hvals collections:hvals
   :hlen collections:hlen
   :hmset collections:hmset
   :hmget collections:hmget
   :hincrby collections:hincrby

   # Commands — List
   :lpush collections:lpush
   :rpush collections:rpush
   :lpop collections:lpop
   :rpop collections:rpop
   :llen collections:llen
   :lrange collections:lrange
   :lindex collections:lindex
   :lset collections:lset

   # Commands — Set
   :sadd collections:sadd
   :srem collections:srem
   :sismember collections:sismember
   :smembers collections:smembers
   :scard collections:scard
   :sunion collections:sunion
   :sinter collections:sinter
   :sdiff collections:sdiff

   # Commands — Sorted Set
   :zadd zset:zadd
   :zscore zset:zscore
   :zrank zset:zrank
   :zrange zset:zrange
   :zrangebyscore zset:zrangebyscore
   :zrem zset:zrem
   :zcard zset:zcard
   :zincrby zset:zincrby
   :zcount zset:zcount
   :zrevrange zset:zrevrange
   :zrevrangebyscore zset:zrevrangebyscore
   :zrange-withscores zset:zrange-withscores
   :zrangebyscore-withscores zset:zrangebyscore-withscores
   :zrevrange-withscores zset:zrevrange-withscores

   # Commands — Server
   :ping server:ping
   :echo server:echo
   :select server:select
   :flushdb server:flushdb
   :dbsize server:dbsize
   :info server:info

   # Transactions
   :multi server:multi
   :exec server:exec
   :discard server:discard
   :watch server:watch
   :unwatch server:unwatch
   :atomic server:atomic

   # Lua scripting
   :eval server:eval
   :evalsha server:evalsha
   :script-load server:script-load
   :script-exists server:script-exists
   :script-flush server:script-flush

   # Pub/Sub
   :subscribe connection:subscribe
   :recv connection:recv
   :unsubscribe connection:unsubscribe
   :publish connection:publish
   :psubscribe connection:psubscribe
   :punsubscribe connection:punsubscribe

   # Pipelining
   :pipeline connection:pipeline

   # RESP primitives (for advanced use)
   :resp-encode resp:encode
   :resp-read resp:read
   :resp-read-raw resp:read-raw

   # Internal tests
   :test resp:test})
