(elle/epoch 9)
## lib/redis.lisp — Pure Elle Redis client (RESP2)
##
## Loaded via: (def redis ((import "std/redis")))
## Usage:      (redis:set "key" "value")
##             (redis:get "key")
##
## Connection model: bare TCP port, no wrapper struct.
## Error model: manager fiber owns the port, reconnects on transient errors.
## Protocol: RESP2 over TCP.

## ── RESP2 encoder ─────────────────────────────────────────────────────

(defn resp-encode [& args]
  "Encode a Redis command as a RESP2 array of bulk strings.
   Each argument is converted to a string and sent as a bulk string."
  (def buf @"")
  (push buf (string "*" (length args) "\r\n"))
  (each arg in args
    (let [s (string arg)]
      (push buf (string "$" (string/size-of s) "\r\n" s "\r\n"))))
  (freeze buf))

## ── RESP2 decoder ─────────────────────────────────────────────────────

(defn resp-read [port]
  "Read a single RESP2 reply from port. Signals on error replies.
   Returns Elle values: string, integer, array, or nil."
  (let [line (port/read-line port)]
    (when (nil? line)
      (error {:error :redis-error
              :reason :unexpected-eof
              :message "unexpected EOF"}))
    (let [prefix (get line 0)
          body (slice line 1)]
      (case
        prefix  # Simple string
        "+" body

        # Error
        "-" (error {:error :redis-error
                    :reason :server-error
                    :body body
                    :message body})

        # Integer
        ":" (parse-int body)

        # Bulk string
        "$"
          (let [len (parse-int body)]
            (if (= len -1)
              nil
              (let [data (port/read port (+ len 2))]
                (when (nil? data)
                  (error {:error :redis-error
                          :reason :unexpected-eof
                          :phase :bulk-string
                          :message "unexpected EOF reading bulk string"}))
                (string (slice data 0 len)))))

        # Array
        "*"
          (let [count (parse-int body)]
            (if (= count -1)
              nil
              (block (def result @[])
                (def @i 0)
                (while (< i count)
                  (push result (resp-read port))
                  (assign i (+ i 1)))
                (freeze result))))
        (error {:error :redis-error
                :reason :unexpected-prefix
                :prefix prefix
                :message (string "unexpected RESP prefix: " prefix)})))))

(defn resp-read-raw [port]
  "Read a RESP2 reply, returning errors as structs instead of signaling.
   Used for pipelining where one bad reply shouldn't corrupt state."
  (let [[ok? val] (protect (resp-read port))]
    (if ok? val val)))

## ── Command helpers ───────────────────────────────────────────────────

(defn resp-ok? [val]
  "Convert 'OK' string replies to true."
  (if (= val "OK") true val))

(defn resp-bool [val]
  "Convert integer 0/1 replies to boolean."
  (= val 1))

## ── Parameter for ambient port access ─────────────────────────────────

(def *redis-port* (parameter nil))

(defn redis-cmd [& args]
  "Send a command on the current Redis port and read the reply."
  (let [port (*redis-port*)]
    (when (nil? port)
      (error {:error :redis-error
              :reason :no-connection
              :message "no active Redis connection"}))
    (port/write port (apply resp-encode args))
    (port/flush port)
    (resp-read port)))

## ── Connection ────────────────────────────────────────────────────────

(defn redis-connect [host port]
  "Connect to Redis via TCP. Returns the raw TCP port."
  (tcp/connect host port))

(defn redis-close [port]
  "Close a Redis connection."
  (port/close port))

(defn redis-auth [& args]
  "AUTH [username] password — authenticate with Redis.
   (redis:auth \"password\") or (redis:auth \"user\" \"password\")."
  (resp-ok? (apply redis-cmd (pair "AUTH" args))))

## ── Manager fiber ─────────────────────────────────────────────────────

(defn default-terminal? [err]
  "Default predicate for terminal errors. Auth failures and protocol
   corruption are terminal; everything else is retryable."
  (let [kind (get err :error)]
    (or (= kind :auth-error) (= kind :protocol-error))))

(defn redis-manager [host port &named terminal? max-retries]
  "Create a manager fiber that owns a Redis connection.
   Reconnects on non-terminal errors; crashes on terminal ones.
   Returns a struct {:run run :port-param *redis-port*}
   where run is a function that takes a thunk and executes it
   with the managed connection."
  (let [is-terminal (or terminal? default-terminal?)
        retries (or max-retries 3)
        param (parameter nil)]
    (defn run-with-manager [thunk]
      "Execute thunk with a managed Redis connection."
      (def @conn (redis-connect host port))
      (def @attempts 0)
      (defer
        (port/close conn)
        (def @result nil)
        (def @done false)
        (while (not done)
          (let [[ok? val] (protect (parameterize ((param conn))
                                     (thunk)))]
            (if ok?
              (begin
                (assign result val)
                (assign done true))
              (if (is-terminal val)
                (error val)
                (begin
                  (assign attempts (+ attempts 1))
                  (when (>= attempts retries) (error val))  # Reconnect
                  (let [[close-ok? _] (protect (port/close conn))])
                  (assign conn (redis-connect host port)))))))))

    {:run run-with-manager :port-param param}))

## ── Client — simplified connection for direct use ─────────────────────

(defn redis-with [host port thunk]
  "Open a Redis connection, run thunk with *redis-port* bound, close on exit."
  (let [conn (redis-connect host port)]
    (defer
      (port/close conn)
      (parameterize ((*redis-port* conn))
        (thunk)))))

## ── Commands — String ─────────────────────────────────────────────────

(defn redis-get [key]
  "GET key — returns string or nil."
  (redis-cmd "GET" key))

(defn redis-set [key value &named ex px nx xx]
  "SET key value [EX seconds] [PX ms] [NX|XX] — returns true on OK."
  (def args @["SET" key value])
  (when ex
    (push args "EX")
    (push args (string ex)))
  (when px
    (push args "PX")
    (push args (string px)))
  (when nx (push args "NX"))
  (when xx (push args "XX"))
  (resp-ok? (apply redis-cmd (freeze args))))

(defn redis-mget [& keys]
  "MGET key [key ...] — returns array of values."
  (apply redis-cmd (pair "MGET" keys)))

(defn redis-mset [& pairs]
  "MSET key value [key value ...] — returns true on OK."
  (resp-ok? (apply redis-cmd (pair "MSET" pairs))))

(defn redis-incr [key]
  "INCR key — returns new integer value."
  (redis-cmd "INCR" key))

(defn redis-decr [key]
  "DECR key — returns new integer value."
  (redis-cmd "DECR" key))

(defn redis-incrby [key n]
  "INCRBY key increment — returns new integer value."
  (redis-cmd "INCRBY" key (string n)))

(defn redis-decrby [key n]
  "DECRBY key decrement — returns new integer value."
  (redis-cmd "DECRBY" key (string n)))

(defn redis-append [key value]
  "APPEND key value — returns new length."
  (redis-cmd "APPEND" key value))

(defn redis-strlen [key]
  "STRLEN key — returns length."
  (redis-cmd "STRLEN" key))

(defn redis-getset [key value]
  "GETSET key value — returns old value."
  (redis-cmd "GETSET" key value))

(defn redis-setnx [key value]
  "SETNX key value — returns true if set."
  (resp-bool (redis-cmd "SETNX" key value)))

## ── Commands — Keys ───────────────────────────────────────────────────

(defn redis-del [& keys]
  "DEL key [key ...] — returns count of deleted keys."
  (apply redis-cmd (pair "DEL" keys)))

(defn redis-exists [key]
  "EXISTS key — returns true if exists."
  (resp-bool (redis-cmd "EXISTS" key)))

(defn redis-expire [key seconds]
  "EXPIRE key seconds — returns true if timeout was set."
  (resp-bool (redis-cmd "EXPIRE" key (string seconds))))

(defn redis-ttl [key]
  "TTL key — returns remaining seconds, -1 if no expire, -2 if not exists."
  (redis-cmd "TTL" key))

(defn redis-type [key]
  "TYPE key — returns type string."
  (redis-cmd "TYPE" key))

(defn redis-keys [pattern]
  "KEYS pattern — returns array of matching keys."
  (redis-cmd "KEYS" pattern))

(defn redis-rename [key newkey]
  "RENAME key newkey — returns true on OK."
  (resp-ok? (redis-cmd "RENAME" key newkey)))

(defn redis-persist [key]
  "PERSIST key — remove expiration."
  (resp-bool (redis-cmd "PERSIST" key)))

(defn redis-pexpire [key ms]
  "PEXPIRE key milliseconds — set expiration in ms."
  (resp-bool (redis-cmd "PEXPIRE" key (string ms))))

(defn redis-pttl [key]
  "PTTL key — remaining ms, -1 if no expire, -2 if not exists."
  (redis-cmd "PTTL" key))

(defn redis-expireat [key timestamp]
  "EXPIREAT key unix-time — set expiration as absolute unix timestamp."
  (resp-bool (redis-cmd "EXPIREAT" key (string timestamp))))

(defn redis-pexpireat [key timestamp-ms]
  "PEXPIREAT key unix-time-ms — set expiration as absolute unix timestamp in ms."
  (resp-bool (redis-cmd "PEXPIREAT" key (string timestamp-ms))))

## ── Commands — Scan ───────────────────────────────────────────────────

(defn redis-scan [cursor &named match count]
  "SCAN cursor [MATCH pattern] [COUNT count] — incrementally iterate keys.
   Returns [next-cursor keys-array]. Cursor \"0\" starts; returns \"0\" when done."
  (def args @["SCAN" (string cursor)])
  (when match
    (push args "MATCH")
    (push args match))
  (when count
    (push args "COUNT")
    (push args (string count)))
  (let [result (apply redis-cmd (freeze args))]
    [(get result 0) (get result 1)]))

(defn redis-hscan [key cursor &named match count]
  "HSCAN key cursor [MATCH pattern] [COUNT count] — iterate hash fields.
   Returns [next-cursor flat-array] where flat-array is [field val field val ...]."
  (def args @["HSCAN" key (string cursor)])
  (when match
    (push args "MATCH")
    (push args match))
  (when count
    (push args "COUNT")
    (push args (string count)))
  (let [result (apply redis-cmd (freeze args))]
    [(get result 0) (get result 1)]))

(defn redis-sscan [key cursor &named match count]
  "SSCAN key cursor [MATCH pattern] [COUNT count] — iterate set members.
   Returns [next-cursor members-array]."
  (def args @["SSCAN" key (string cursor)])
  (when match
    (push args "MATCH")
    (push args match))
  (when count
    (push args "COUNT")
    (push args (string count)))
  (let [result (apply redis-cmd (freeze args))]
    [(get result 0) (get result 1)]))

(defn redis-zscan [key cursor &named match count]
  "ZSCAN key cursor [MATCH pattern] [COUNT count] — iterate sorted set members.
   Returns [next-cursor flat-array] where flat-array is [member score member score ...]."
  (def args @["ZSCAN" key (string cursor)])
  (when match
    (push args "MATCH")
    (push args match))
  (when count
    (push args "COUNT")
    (push args (string count)))
  (let [result (apply redis-cmd (freeze args))]
    [(get result 0) (get result 1)]))

(defn redis-scan-all [scan-fn & scan-args]
  "Drain a scan cursor to completion, returning all results as a single array.
   scan-fn: one of redis-scan, redis-hscan, redis-sscan, redis-zscan.
   scan-args: remaining args to scan-fn (excluding cursor).
   Example: (redis:scan-all redis:scan :match \"user:*\")"
  (def acc @[])
  (def @cursor "0")
  (def @first true)
  (while (or first (not (= cursor "0")))
    (assign first false)
    (let [[next-cursor items] (apply scan-fn (pair cursor scan-args))]
      (assign cursor next-cursor)
      (each item in items
        (push acc item))))
  (freeze acc))

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

## ── Commands — Server ─────────────────────────────────────────────────

(defn redis-ping []
  "PING — returns 'PONG'."
  (redis-cmd "PING"))

(defn redis-echo [message]
  "ECHO message — returns the message."
  (redis-cmd "ECHO" message))

(defn redis-select [db]
  "SELECT db — switch database. Returns true on OK."
  (resp-ok? (redis-cmd "SELECT" (string db))))

(defn redis-flushdb []
  "FLUSHDB — delete all keys in current database."
  (resp-ok? (redis-cmd "FLUSHDB")))

(defn redis-dbsize []
  "DBSIZE — returns number of keys."
  (redis-cmd "DBSIZE"))

(defn redis-info [&named section]
  "INFO [section] — returns server info string."
  (if section (redis-cmd "INFO" section) (redis-cmd "INFO")))

## ── Transactions ──────────────────────────────────────────────────────

(defn redis-multi []
  "MULTI — start a transaction. Returns true on OK."
  (resp-ok? (redis-cmd "MULTI")))

(defn redis-exec []
  "EXEC — execute queued transaction commands. Returns array of results,
   or nil if the transaction was aborted (WATCH key changed)."
  (redis-cmd "EXEC"))

(defn redis-discard []
  "DISCARD — abort the current transaction. Returns true on OK."
  (resp-ok? (redis-cmd "DISCARD")))

(defn redis-watch [& keys]
  "WATCH key [key ...] — optimistic locking. If any watched key changes
   before EXEC, the transaction aborts (EXEC returns nil)."
  (resp-ok? (apply redis-cmd (pair "WATCH" keys))))

(defn redis-unwatch []
  "UNWATCH — cancel all watched keys."
  (resp-ok? (redis-cmd "UNWATCH")))

(defn redis-atomic [watch-keys body-fn]
  "Execute body-fn inside a WATCH/MULTI/EXEC retry loop.
   watch-keys: list of keys to WATCH (may be empty).
   body-fn: called with no args. Reads happen normally (before MULTI is
   implicit — body-fn should read first, then call redis:multi, then
   issue write commands). EXEC is called automatically after body-fn returns.
   If WATCH detects a conflict (EXEC returns nil), the whole body-fn is retried.
   Retries up to 16 times before signaling an error.

   Example — CAS increment:
     (redis:atomic [\"counter\"]
       (fn []
         (let [[val (integer (redis:get \"counter\"))]]
           (redis:multi)
           (redis:set \"counter\" (string (+ val 1))))))
   => [true]"
  (def @attempts 0)
  (def @result nil)
  (def @done false)
  (while (not done)
    (when (not (empty? watch-keys)) (apply redis-watch watch-keys))
    (let [[ok? val] (protect (body-fn))]
      (if ok?
        (let [exec-result (redis-exec)]
          (if (nil? exec-result)
            (begin  # WATCH was violated — retry
              (assign attempts (+ attempts 1))
              (when (>= attempts 16)
                (error {:error :redis-error
                        :reason :watch-conflict
                        :message "too many retries (WATCH conflict)"})))
            (begin
              (assign result exec-result)
              (assign done true))))  # body-fn errored — discard and re-raise
        (begin
          (let [[_ _] (protect (redis-discard))])
          (error val)))))
  result)

## ── Lua scripting ─────────────────────────────────────────────────────

(defn redis-eval [script numkeys & args]
  "EVAL script numkeys [key ...] [arg ...] — execute a Lua script.
   Returns the script's return value."
  (apply redis-cmd (pair "EVAL" (pair script (pair (string numkeys) args)))))

(defn redis-evalsha [sha numkeys & args]
  "EVALSHA sha1 numkeys [key ...] [arg ...] — execute a cached Lua script."
  (apply redis-cmd (pair "EVALSHA" (pair sha (pair (string numkeys) args)))))

(defn redis-script-load [script]
  "SCRIPT LOAD script — load a script into cache, returns SHA1."
  (redis-cmd "SCRIPT" "LOAD" script))

(defn redis-script-exists [& shas]
  "SCRIPT EXISTS sha1 [sha1 ...] — check if scripts are cached.
   Returns array of 0/1 integers."
  (apply redis-cmd (pair "SCRIPT" (pair "EXISTS" shas))))

(defn redis-script-flush []
  "SCRIPT FLUSH — clear the script cache."
  (resp-ok? (redis-cmd "SCRIPT" "FLUSH")))

## ── Pub/Sub ───────────────────────────────────────────────────────────

(defn redis-subscribe [port & channels]
  "Send SUBSCRIBE command on port. Returns the port (now in sub mode)."
  (port/write port (apply resp-encode (pair "SUBSCRIBE" channels)))
  (port/flush port)  # Read the subscription confirmations
  (each ch in channels
    (resp-read port))
  port)

(defn redis-recv [port]
  "Read next pub/sub message from a subscribed port.
   Returns {:channel string :data string} or nil on EOF."
  (let [[ok? msg] (protect (resp-read port))]
    (if (not ok?)
      nil
      (if (and (array? msg) (>= (length msg) 3) (= (get msg 0) "message"))
        {"channel" (get msg 1) "data" (get msg 2)}
        nil))))

(defn redis-unsubscribe [port & channels]
  "Send UNSUBSCRIBE command on port."
  (port/write port (apply resp-encode (pair "UNSUBSCRIBE" channels)))
  (port/flush port)  # Read the unsubscription confirmations
  (each ch in channels
    (resp-read port)))

(defn redis-publish [channel message]
  "PUBLISH channel message — returns number of receivers."
  (redis-cmd "PUBLISH" channel message))

(defn redis-psubscribe [port & patterns]
  "Send PSUBSCRIBE command on port."
  (port/write port (apply resp-encode (pair "PSUBSCRIBE" patterns)))
  (port/flush port)
  (each p in patterns
    (resp-read port))
  port)

(defn redis-punsubscribe [port & patterns]
  "Send PUNSUBSCRIBE command on port."
  (port/write port (apply resp-encode (pair "PUNSUBSCRIBE" patterns)))
  (port/flush port)
  (each p in patterns
    (resp-read port)))

## ── Pipelining ────────────────────────────────────────────────────────

(defn redis-pipeline [& commands]
  "Send multiple commands in a batch, read all replies.
   Each command is a list like (list 'GET' 'key').
   Uses resp-read-raw so error replies don't corrupt state.
   Returns array of results."
  (let [port (*redis-port*)]
    (when (nil? port)
      (error {:error :redis-error
              :reason :no-connection
              :message "no active Redis connection"}))  # Send all commands
    (each cmd in commands
      (port/write port (apply resp-encode cmd)))
    (port/flush port)  # Read all replies
    (def results @[])
    (each cmd in commands
      (push results (resp-read-raw port)))
    (freeze results)))

## ── Internal self-tests (RESP encoding/decoding, no Redis needed) ─────

(defn run-internal-tests []
  "Self-tests for RESP encoding and decoding."

  # resp-encode
  (assert (= (resp-encode "PING") "*1\r\n$4\r\nPING\r\n") "resp-encode PING")

  (assert (= (resp-encode "SET" "key" "value")
             "*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n")
          "resp-encode SET key value")

  (assert (= (resp-encode "GET" "mykey") "*2\r\n$3\r\nGET\r\n$5\r\nmykey\r\n")
          "resp-encode GET mykey")

  # resp-encode with multibyte string
  (assert (= (resp-encode "SET" "k" "café")
             "*3\r\n$3\r\nSET\r\n$1\r\nk\r\n$5\r\ncafé\r\n")
          "resp-encode multibyte uses byte length")

  # resp-read: simple string
  (spit "/tmp/elle-redis-test-simple" "+OK\r\n")
  (let [p (port/open "/tmp/elle-redis-test-simple" :read)]
    (defer
      (port/close p)
      (assert (= (resp-read p) "OK") "resp-read simple string")))

  # resp-read: integer
  (spit "/tmp/elle-redis-test-int" ":42\r\n")
  (let [p (port/open "/tmp/elle-redis-test-int" :read)]
    (defer
      (port/close p)
      (assert (= (resp-read p) 42) "resp-read integer")))

  # resp-read: bulk string
  (spit "/tmp/elle-redis-test-bulk" "$5\r\nhello\r\n")
  (let [p (port/open "/tmp/elle-redis-test-bulk" :read)]
    (defer
      (port/close p)
      (assert (= (resp-read p) "hello") "resp-read bulk string")))

  # resp-read: nil bulk string
  (spit "/tmp/elle-redis-test-nil" "$-1\r\n")
  (let [p (port/open "/tmp/elle-redis-test-nil" :read)]
    (defer
      (port/close p)
      (assert (nil? (resp-read p)) "resp-read nil bulk string")))

  # resp-read: array
  (spit "/tmp/elle-redis-test-arr" "*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n")
  (let [p (port/open "/tmp/elle-redis-test-arr" :read)]
    (defer
      (port/close p)
      (let [result (resp-read p)]
        (assert (= (length result) 2) "resp-read array length")
        (assert (= (get result 0) "foo") "resp-read array element 0")
        (assert (= (get result 1) "bar") "resp-read array element 1"))))

  # resp-read: error
  (spit "/tmp/elle-redis-test-err" "-ERR unknown command\r\n")
  (let [p (port/open "/tmp/elle-redis-test-err" :read)]
    (defer
      (port/close p)
      (let [[ok? val] (protect (resp-read p))]
        (assert (not ok?) "resp-read error signals")
        (assert (= (get val :error) :redis-error) "resp-read error kind")
        (assert (= (get val :message) "ERR unknown command")
                "resp-read error message"))))

  # resp-read-raw: error returns struct instead of signaling
  (spit "/tmp/elle-redis-test-raw-err" "-ERR bad\r\n")
  (let [p (port/open "/tmp/elle-redis-test-raw-err" :read)]
    (defer
      (port/close p)
      (let [result (resp-read-raw p)]
        (assert (struct? result) "resp-read-raw error is struct")
        (assert (= (get result :error) :redis-error) "resp-read-raw error kind"))))

  # resp-ok?
  (assert (= (resp-ok? "OK") true) "resp-ok? OK")
  (assert (= (resp-ok? "QUEUED") "QUEUED") "resp-ok? passthrough")

  # resp-bool
  (assert (= (resp-bool 1) true) "resp-bool 1")
  (assert (= (resp-bool 0) false) "resp-bool 0")

  # resp-read: nested array
  (spit "/tmp/elle-redis-test-nested"
        "*2\r\n*2\r\n:1\r\n:2\r\n*2\r\n:3\r\n:4\r\n")
  (let [p (port/open "/tmp/elle-redis-test-nested" :read)]
    (defer
      (port/close p)
      (let [result (resp-read p)]
        (assert (= (length result) 2) "nested array length")
        (assert (= (get (get result 0) 0) 1) "nested array [0][0]")
        (assert (= (get (get result 1) 1) 4) "nested array [1][1]"))))

  # resp-read: empty array
  (spit "/tmp/elle-redis-test-empty-arr" "*0\r\n")
  (let [p (port/open "/tmp/elle-redis-test-empty-arr" :read)]
    (defer
      (port/close p)
      (let [result (resp-read p)]
        (assert (= (length result) 0) "empty array"))))

  true)

## ── Exports ───────────────────────────────────────────────────────────

(fn []
  {
   # Connection
   :connect redis-connect
   :close redis-close
   :with redis-with
   :auth redis-auth

   # Manager
   :manager redis-manager

   # Commands — String
   :get redis-get
   :set redis-set
   :mget redis-mget
   :mset redis-mset
   :incr redis-incr
   :decr redis-decr
   :incrby redis-incrby
   :decrby redis-decrby
   :append redis-append
   :strlen redis-strlen
   :getset redis-getset
   :setnx redis-setnx

   # Commands — Keys
   :del redis-del
   :exists redis-exists
   :expire redis-expire
   :pexpire redis-pexpire
   :expireat redis-expireat
   :pexpireat redis-pexpireat
   :ttl redis-ttl
   :pttl redis-pttl
   :type redis-type
   :keys redis-keys
   :rename redis-rename
   :persist redis-persist

   # Commands — Scan
   :scan redis-scan
   :hscan redis-hscan
   :sscan redis-sscan
   :zscan redis-zscan
   :scan-all redis-scan-all

   # Commands — Hash
   :hset redis-hset
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

   # Commands — List
   :lpush redis-lpush
   :rpush redis-rpush
   :lpop redis-lpop
   :rpop redis-rpop
   :llen redis-llen
   :lrange redis-lrange
   :lindex redis-lindex
   :lset redis-lset

   # Commands — Set
   :sadd redis-sadd
   :srem redis-srem
   :sismember redis-sismember
   :smembers redis-smembers
   :scard redis-scard
   :sunion redis-sunion
   :sinter redis-sinter
   :sdiff redis-sdiff

   # Commands — Sorted Set
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
   :zrevrange-withscores redis-zrevrange-withscores

   # Commands — Server
   :ping redis-ping
   :echo redis-echo
   :select redis-select
   :flushdb redis-flushdb
   :dbsize redis-dbsize
   :info redis-info

   # Transactions
   :multi redis-multi
   :exec redis-exec
   :discard redis-discard
   :watch redis-watch
   :unwatch redis-unwatch
   :atomic redis-atomic

   # Lua scripting
   :eval redis-eval
   :evalsha redis-evalsha
   :script-load redis-script-load
   :script-exists redis-script-exists
   :script-flush redis-script-flush

   # Pub/Sub
   :subscribe redis-subscribe
   :recv redis-recv
   :unsubscribe redis-unsubscribe
   :publish redis-publish
   :psubscribe redis-psubscribe
   :punsubscribe redis-punsubscribe

   # Pipelining
   :pipeline redis-pipeline

   # RESP primitives (for advanced use)
   :resp-encode resp-encode
   :resp-read resp-read
   :resp-read-raw resp-read-raw

   # Internal tests
   :test run-internal-tests})
