## lib/redis.lisp — Pure Elle Redis client (RESP2 over TCP)
##
## Loaded via: (def redis ((import-file "lib/redis.lisp")))
## Usage:      (redis:get conn "key")

# ============================================================================
# RESP2 encoder
# ============================================================================

(defn resp-encode [args]
  "Encode an array of strings as a RESP2 array of bulk strings.
   Returns a single string ready to write to a port."
  (let [[parts @[]]]
    (push parts (string/format "*{}\r\n" (length args)))
    (each arg in args
      (push parts (string/format "${}\r\n{}\r\n"
                                 (string/size-of arg) arg)))
    (string/join (freeze parts) "")))

# ============================================================================
# RESP2 decoder
# ============================================================================

(defn resp-read-exact [port n]
  "Read exactly n bytes from port, looping on short reads.
   Signals :redis-error on unexpected EOF."
  (var remaining n)
  (var parts @[])
  (while (> remaining 0)
    (let [[chunk (stream/read port remaining)]]
      (when (nil? chunk)
        (error {:error :redis-error
                :message (string/format "unexpected EOF: needed {} more bytes"
                                        remaining)}))
      (push parts chunk)
      (assign remaining (- remaining (string/size-of chunk)))))
  (string/join (freeze parts) ""))

(defn resp-read [port]
  "Read one RESP2 value from port. Returns the parsed Elle value.
   Signals :redis-error on protocol errors or Redis error replies."
  (let [[line (stream/read-line port)]]
    (when (nil? line)
      (error {:error :redis-error :message "unexpected EOF from Redis"}))
    (let [[type-char (first line)]
          [payload   (rest line)]]
      (case type-char
        "+" payload
        "-" (error {:error :redis-error :message payload})
        ":" (integer payload)
        "$" (let [[n (integer payload)]]
              (if (= n -1)
                nil
                (let [[data (resp-read-exact port n)]]
                  (stream/read-line port)
                  data)))
        "*" (let [[n (integer payload)]]
              (if (= n -1)
                nil
                (let [[arr @[]]]
                  (var i 0)
                  (while (< i n)
                    (push arr (resp-read port))
                    (assign i (+ i 1)))
                  (freeze arr))))
        (error {:error :redis-error
                :message (string/format "unknown RESP type: {}" type-char)})))))

(defn resp-read-raw [port]
  "Like resp-read but returns Redis error replies as
   {:error :redis-error :message ...} values instead of signaling."
  (let [[line (stream/read-line port)]]
    (when (nil? line)
      (error {:error :redis-error :message "unexpected EOF from Redis"}))
    (let [[type-char (first line)]
          [payload   (rest line)]]
      (case type-char
        "+" payload
        "-" {:error :redis-error :message payload}
        ":" (integer payload)
        "$" (let [[n (integer payload)]]
              (if (= n -1)
                nil
                (let [[data (resp-read-exact port n)]]
                  (stream/read-line port)
                  data)))
        "*" (let [[n (integer payload)]]
              (if (= n -1)
                nil
                (let [[arr @[]]]
                  (var i 0)
                  (while (< i n)
                    (push arr (resp-read-raw port))
                    (assign i (+ i 1)))
                  (freeze arr))))
        (error {:error :redis-error
                :message (string/format "unknown RESP type: {}" type-char)})))))

(defn resp-ok? [val]
  "Convert Redis 'OK' status to true, pass through everything else."
  (if (= val "OK") true val))

# ============================================================================
# Connection management
# ============================================================================

(defn assert-conn [conn name]
  (unless (and (struct? conn) (= conn:type :redis-conn))
    (error {:error :type-error
            :message (string/format "redis/{}: expected redis connection, got {}"
                                    name (type conn))})))

(defn redis-command [conn args]
  "Send a RESP command and read one reply."
  (assert-conn conn "command")
  (stream/write conn:port (resp-encode args))
  (stream/flush conn:port)
  (resp-read conn:port))

(defn redis-connect [host port &named auth db]
  "Connect to Redis at host:port. Optional :auth and :db."
  (let* [[sock (tcp/connect host port)]
         [conn {:port sock :type :redis-conn}]]
    (when auth
      (redis-command conn ["AUTH" auth]))
    (when db
      (redis-command conn ["SELECT" (string db)]))
    conn))

(defn redis-close [conn]
  "Close a Redis connection."
  (assert-conn conn "close")
  (port/close conn:port))

# ============================================================================
# String commands
# ============================================================================

(defn redis-get [conn key]
  (redis-command conn ["GET" key]))

(defn redis-set [conn key value &named ex px nx xx]
  (let [[args @["SET" key value]]]
    (when ex (push args "EX") (push args (string ex)))
    (when px (push args "PX") (push args (string px)))
    (when nx (push args "NX"))
    (when xx (push args "XX"))
    (resp-ok? (redis-command conn (freeze args)))))

(defn redis-del [conn key]
  (redis-command conn ["DEL" key]))

(defn redis-exists [conn key]
  (= 1 (redis-command conn ["EXISTS" key])))

(defn redis-incr [conn key]
  (redis-command conn ["INCR" key]))

(defn redis-decr [conn key]
  (redis-command conn ["DECR" key]))

(defn redis-expire [conn key seconds]
  (= 1 (redis-command conn ["EXPIRE" key (string seconds)])))

(defn redis-ttl [conn key]
  (redis-command conn ["TTL" key]))

(defn redis-mget [conn keys]
  (redis-command conn (concat ["MGET"] keys)))

(defn redis-mset [conn pairs]
  (resp-ok? (redis-command conn (concat ["MSET"] pairs))))

# ============================================================================
# List commands
# ============================================================================

(defn redis-lpush [conn key value]
  (redis-command conn ["LPUSH" key value]))

(defn redis-rpush [conn key value]
  (redis-command conn ["RPUSH" key value]))

(defn redis-lpop [conn key]
  (redis-command conn ["LPOP" key]))

(defn redis-rpop [conn key]
  (redis-command conn ["RPOP" key]))

(defn redis-lrange [conn key start stop]
  (redis-command conn ["LRANGE" key (string start) (string stop)]))

(defn redis-llen [conn key]
  (redis-command conn ["LLEN" key]))

# ============================================================================
# Hash commands
# ============================================================================

(defn redis-hset [conn key field value]
  (redis-command conn ["HSET" key field value]))

(defn redis-hget [conn key field]
  (redis-command conn ["HGET" key field]))

(defn redis-hgetall [conn key]
  (let [[arr (redis-command conn ["HGETALL" key])]
        [result @{}]]
    (var i 0)
    (while (< i (length arr))
      (put result (get arr i) (get arr (+ i 1)))
      (assign i (+ i 2)))
    (freeze result)))

(defn redis-hdel [conn key field]
  (redis-command conn ["HDEL" key field]))

(defn redis-hexists [conn key field]
  (= 1 (redis-command conn ["HEXISTS" key field])))

(defn redis-hlen [conn key]
  (redis-command conn ["HLEN" key]))

(defn redis-hmset [conn key pairs]
  (resp-ok? (redis-command conn (concat ["HMSET" key] pairs))))

(defn redis-hmget [conn key fields]
  (redis-command conn (concat ["HMGET" key] fields)))

# ============================================================================
# Set commands
# ============================================================================

(defn redis-sadd [conn key member]
  (redis-command conn ["SADD" key member]))

(defn redis-srem [conn key member]
  (redis-command conn ["SREM" key member]))

(defn redis-smembers [conn key]
  (redis-command conn ["SMEMBERS" key]))

(defn redis-scard [conn key]
  (redis-command conn ["SCARD" key]))

(defn redis-sismember [conn key member]
  (= 1 (redis-command conn ["SISMEMBER" key member])))

# ============================================================================
# Sorted set commands
# ============================================================================

(defn redis-zadd [conn key score member]
  (redis-command conn ["ZADD" key (string score) member]))

(defn redis-zrange [conn key start stop]
  (redis-command conn ["ZRANGE" key (string start) (string stop)]))

(defn redis-zrank [conn key member]
  (redis-command conn ["ZRANK" key member]))

(defn redis-zscore [conn key member]
  (redis-command conn ["ZSCORE" key member]))

(defn redis-zrem [conn key member]
  (redis-command conn ["ZREM" key member]))

(defn redis-zcard [conn key]
  (redis-command conn ["ZCARD" key]))

# ============================================================================
# Admin commands
# ============================================================================

(defn redis-keys [conn pattern]
  (redis-command conn ["KEYS" pattern]))

(defn redis-flushdb [conn]
  (resp-ok? (redis-command conn ["FLUSHDB"])))

(defn redis-ping [conn]
  (redis-command conn ["PING"]))

(defn redis-select [conn db]
  (resp-ok? (redis-command conn ["SELECT" (string db)])))

(defn redis-auth [conn password]
  (resp-ok? (redis-command conn ["AUTH" password])))

(defn redis-dbsize [conn]
  (redis-command conn ["DBSIZE"]))

(defn redis-info [conn]
  (redis-command conn ["INFO"]))

(defn redis-publish [conn channel message]
  (redis-command conn ["PUBLISH" channel message]))

# ============================================================================
# Pub/sub
# ============================================================================

(defn redis-subscribe [conn channel handler]
  "Subscribe to a channel and run handler for each message.
   Blocks until unsubscribe or connection closes."
  (assert-conn conn "subscribe")
  (stream/write conn:port (resp-encode ["SUBSCRIBE" channel]))
  (stream/flush conn:port)
  # Read and validate subscription confirmation
  (let [[confirm (resp-read-raw conn:port)]]
    (unless (and (array? confirm)
                 (>= (length confirm) 3)
                 (= (get confirm 0) "subscribe"))
      (error {:error :redis-error
              :message (string/format "unexpected subscribe reply: {}" confirm)})))
  # Message loop
  (forever
    (let [[[ok? reply] (protect (resp-read conn:port))]]
      (unless ok? (break nil))
      (when (nil? reply) (break nil))
      (unless (and (array? reply) (>= (length reply) 3))
        (error {:error :redis-error
                :message (string/format "malformed pub/sub message: {}" reply)}))
      (let [[kind (get reply 0)]]
        (case kind
          "message"     (handler {"channel" (get reply 1)
                                  "data"    (get reply 2)})
          "pmessage"    (handler {"pattern" (get reply 1)
                                  "channel" (get reply 2)
                                  "data"    (get reply 3)})
          "subscribe"   nil
          "unsubscribe" (break nil)
          (error {:error :redis-error
                  :message (string/format "unknown pub/sub type: {}" kind)}))))))

(defn redis-psubscribe [conn pattern handler]
  "Subscribe to a pattern and run handler for each message.
   Blocks until punsubscribe or connection closes."
  (assert-conn conn "psubscribe")
  (stream/write conn:port (resp-encode ["PSUBSCRIBE" pattern]))
  (stream/flush conn:port)
  (let [[confirm (resp-read-raw conn:port)]]
    (unless (and (array? confirm)
                 (>= (length confirm) 3)
                 (= (get confirm 0) "psubscribe"))
      (error {:error :redis-error
              :message (string/format "unexpected psubscribe reply: {}" confirm)})))
  (forever
    (let [[[ok? reply] (protect (resp-read conn:port))]]
      (unless ok? (break nil))
      (when (nil? reply) (break nil))
      (unless (and (array? reply) (>= (length reply) 3))
        (error {:error :redis-error
                :message (string/format "malformed pub/sub message: {}" reply)}))
      (let [[kind (get reply 0)]]
        (case kind
          "message"      (handler {"channel" (get reply 1)
                                   "data"    (get reply 2)})
          "pmessage"     (handler {"pattern" (get reply 1)
                                   "channel" (get reply 2)
                                   "data"    (get reply 3)})
          "psubscribe"   nil
          "punsubscribe" (break nil)
          (error {:error :redis-error
                  :message (string/format "unknown pub/sub type: {}" kind)}))))))

# ============================================================================
# Pipelining
# ============================================================================

(defn redis-pipeline [conn commands]
  "Send multiple commands without waiting, then read all replies.
   Uses resp-read-raw so per-command errors don't corrupt the connection."
  (assert-conn conn "pipeline")
  (each cmd in commands
    (stream/write conn:port (resp-encode cmd)))
  (stream/flush conn:port)
  (let [[results @[]]]
    (each _ in commands
      (push results (resp-ok? (resp-read-raw conn:port))))
    (freeze results)))

# ============================================================================
# Internal tests (no Redis required)
# ============================================================================

(defn run-internal-tests []
  "Self-tests on RESP encoding/decoding. Called via (redis:test)."

  # --- resp-encode ---

  (assert (= (resp-encode ["PING"])
             "*1\r\n$4\r\nPING\r\n")
    "resp-encode PING")

  (assert (= (resp-encode ["SET" "key" "value"])
             "*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n")
    "resp-encode SET key value")

  (assert (= (resp-encode [])
             "*0\r\n")
    "resp-encode empty args")

  # Multi-byte UTF-8: "café" is 5 bytes (é = 2 bytes)
  (let [[encoded (resp-encode ["SET" "k" "café"])]]
    (assert (string-contains? encoded "$5\r\n")
      "resp-encode uses byte length for multi-byte"))

  # --- resp-ok? ---

  (assert (= (resp-ok? "OK") true) "resp-ok? OK -> true")
  (assert (= (resp-ok? "PONG") "PONG") "resp-ok? PONG -> PONG")
  (assert (= (resp-ok? nil) nil) "resp-ok? nil -> nil")

  # --- resp-read: simple string ---

  (spit "/tmp/elle-redis-test-simple" "+OK\r\n")
  (let [[p (port/open "/tmp/elle-redis-test-simple" :read)]]
    (let [[val (ev/spawn (fn [] (resp-read p)))]]
      (port/close p)
      (assert (= val "OK") "resp-read simple string")))

  # --- resp-read: integer ---

  (spit "/tmp/elle-redis-test-int" ":42\r\n")
  (let [[p (port/open "/tmp/elle-redis-test-int" :read)]]
    (let [[val (ev/spawn (fn [] (resp-read p)))]]
      (port/close p)
      (assert (= val 42) "resp-read integer")))

  # --- resp-read: negative integer ---

  (spit "/tmp/elle-redis-test-neg" ":-1\r\n")
  (let [[p (port/open "/tmp/elle-redis-test-neg" :read)]]
    (let [[val (ev/spawn (fn [] (resp-read p)))]]
      (port/close p)
      (assert (= val -1) "resp-read negative integer")))

  # --- resp-read: bulk string ---

  (spit "/tmp/elle-redis-test-bulk" "$5\r\nhello\r\n")
  (let [[p (port/open "/tmp/elle-redis-test-bulk" :read)]]
    (let [[val (ev/spawn (fn [] (resp-read p)))]]
      (port/close p)
      (assert (= val "hello") "resp-read bulk string")))

  # --- resp-read: nil bulk string ---

  (spit "/tmp/elle-redis-test-nil" "$-1\r\n")
  (let [[p (port/open "/tmp/elle-redis-test-nil" :read)]]
    (let [[val (ev/spawn (fn [] (resp-read p)))]]
      (port/close p)
      (assert (nil? val) "resp-read nil bulk string")))

  # --- resp-read: bulk string with embedded newline ---

  (spit "/tmp/elle-redis-test-embed" "$11\r\nhello\nworld\r\n")
  (let [[p (port/open "/tmp/elle-redis-test-embed" :read)]]
    (let [[val (ev/spawn (fn [] (resp-read p)))]]
      (port/close p)
      (assert (= val "hello\nworld") "resp-read bulk string with embedded newline")))

  # --- resp-read: array ---

  (spit "/tmp/elle-redis-test-arr" "*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n")
  (let [[p (port/open "/tmp/elle-redis-test-arr" :read)]]
    (let [[val (ev/spawn (fn [] (resp-read p)))]]
      (port/close p)
      (assert (= (length val) 2) "resp-read array length")
      (assert (= (get val 0) "foo") "resp-read array[0]")
      (assert (= (get val 1) "bar") "resp-read array[1]")))

  # --- resp-read: empty array ---

  (spit "/tmp/elle-redis-test-empty-arr" "*0\r\n")
  (let [[p (port/open "/tmp/elle-redis-test-empty-arr" :read)]]
    (let [[val (ev/spawn (fn [] (resp-read p)))]]
      (port/close p)
      (assert (= (length val) 0) "resp-read empty array")))

  # --- resp-read: nil array ---

  (spit "/tmp/elle-redis-test-nil-arr" "*-1\r\n")
  (let [[p (port/open "/tmp/elle-redis-test-nil-arr" :read)]]
    (let [[val (ev/spawn (fn [] (resp-read p)))]]
      (port/close p)
      (assert (nil? val) "resp-read nil array")))

  # --- resp-read: error signals ---

  (spit "/tmp/elle-redis-test-err" "-ERR bad command\r\n")
  (let [[p (port/open "/tmp/elle-redis-test-err" :read)]]
    (let [[[ok? err] (protect (ev/spawn (fn [] (resp-read p))))]]
      (port/close p)
      (assert (not ok?) "resp-read error signals")
      (assert (= err:error :redis-error) "resp-read error kind")))

  # --- resp-read-raw: error returns struct ---

  (spit "/tmp/elle-redis-test-raw-err" "-ERR bad command\r\n")
  (let [[p (port/open "/tmp/elle-redis-test-raw-err" :read)]]
    (let [[val (ev/spawn (fn [] (resp-read-raw p)))]]
      (port/close p)
      (assert (= val:error :redis-error) "resp-read-raw error returns struct")
      (assert (= val:message "ERR bad command") "resp-read-raw error message")))

  # --- resp-encode round-trip ---

  (let [[encoded (resp-encode ["GET" "mykey"])]]
    (spit "/tmp/elle-redis-test-roundtrip" encoded)
    (let [[p (port/open "/tmp/elle-redis-test-roundtrip" :read)]]
      (let [[val (ev/spawn (fn [] (resp-read p)))]]
        (port/close p)
        (assert (= (length val) 2) "round-trip array length")
        (assert (= (get val 0) "GET") "round-trip arg 0")
        (assert (= (get val 1) "mykey") "round-trip arg 1"))))

  true)

# ============================================================================
# Exports
# ============================================================================

(fn []
  {# Connection
   :connect    redis-connect
   :close      redis-close
   :command    redis-command

   # String commands
   :get        redis-get
   :set        redis-set
   :del        redis-del
   :exists     redis-exists
   :incr       redis-incr
   :decr       redis-decr
   :expire     redis-expire
   :ttl        redis-ttl
   :mget       redis-mget
   :mset       redis-mset

   # List commands
   :lpush      redis-lpush
   :rpush      redis-rpush
   :lpop       redis-lpop
   :rpop       redis-rpop
   :lrange     redis-lrange
   :llen       redis-llen

   # Hash commands
   :hset       redis-hset
   :hget       redis-hget
   :hgetall    redis-hgetall
   :hdel       redis-hdel
   :hexists    redis-hexists
   :hlen       redis-hlen
   :hmset      redis-hmset
   :hmget      redis-hmget

   # Set commands
   :sadd       redis-sadd
   :srem       redis-srem
   :smembers   redis-smembers
   :scard      redis-scard
   :sismember  redis-sismember

   # Sorted set commands
   :zadd       redis-zadd
   :zrange     redis-zrange
   :zrank      redis-zrank
   :zscore     redis-zscore
   :zrem       redis-zrem
   :zcard      redis-zcard

   # Admin
   :keys       redis-keys
   :flushdb    redis-flushdb
   :ping       redis-ping
   :select     redis-select
   :auth       redis-auth
   :dbsize     redis-dbsize
   :info       redis-info
   :publish    redis-publish

   # Pub/sub
   :subscribe  redis-subscribe
   :psubscribe redis-psubscribe

   # Pipelining
   :pipeline   redis-pipeline

   # Internal tests
   :test       run-internal-tests})
