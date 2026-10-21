(elle/epoch 15)
# audited: 2026-10-20
## String, key and SCAN commands, each one redis-cmd with its reply converted.
## lib/redis/overview.md
##
## Loaded via:
##   (def keyspace ((import "std/redis/keyspace") :connection connection :resp resp))
##
## Exports: {:get :set :mget :mset :incr :decr :incrby :decrby :append :strlen
##           :getset :setnx :del :exists :expire :ttl :type :keys :rename
##           :persist :pexpire :pttl :expireat :pexpireat
##           :scan :hscan :sscan :zscan :scan-all}

(fn [&named connection resp]
  (def redis-cmd connection:cmd)
  (def resp-ok? resp:ok?)
  (def resp-bool resp:bool)

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

  {:get redis-get
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
   :del redis-del
   :exists redis-exists
   :expire redis-expire
   :ttl redis-ttl
   :type redis-type
   :keys redis-keys
   :rename redis-rename
   :persist redis-persist
   :pexpire redis-pexpire
   :pttl redis-pttl
   :expireat redis-expireat
   :pexpireat redis-pexpireat
   :scan redis-scan
   :hscan redis-hscan
   :sscan redis-sscan
   :zscan redis-zscan
   :scan-all redis-scan-all})
