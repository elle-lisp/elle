(elle/epoch 15)
# audited: 2026-11-12
## The ambient connection: *redis-conn* and the lock beside it, cmd, using, with, manager, pipeline, and pub/sub.
## docs/stdlib/redis/overview.md
##
## Loaded via: (def connection ((import "std/redis/connection") :resp resp))
##
## Concurrency: redis:using, and so redis:with, binds a lock beside the
## connection.  Every redis:cmd and redis:pipeline holds that lock around its
## write and its reads, so callers that share one connection across fibers
## (an http2:serve handler fiber per request, say) cannot interleave RESP
## framing on the wire.  Without the lock, two fibers' pipelines each write
## and then race on the reads, and the RESP parser reads the middle of a
## bulk string as the next length prefix.
##
## Exports: {:cmd :auth :manager :using :with :pipeline
##           :subscribe :recv :unsubscribe :publish :psubscribe :punsubscribe}

(def sync ((import "std/sync")))

## ── Parameter for ambient connection access ───────────────────────────

(def *redis-conn* (parameter nil))

## The lock paired with *redis-conn*, bound by redis-using.  Nil means no
## lock: a caller that owns the connection from one fiber leaves it unbound.
(def *redis-lock* (parameter nil))

(defn with-redis-lock [thunk]
  "Run thunk holding the ambient *redis-lock* (no-op if unbound).
   Released even on error.  Not reentrant: callers must not nest
   redis-cmd/pipeline inside their own with-redis-lock block."
  (let [lk (*redis-lock*)]
    (if (nil? lk)
      (thunk)
      (begin
        ((get lk :acquire))
        (defer
          ((get lk :release))
          (thunk))))))

(defn require-connection [conn where]
  "Replies are read through the connection's framer, so a command needs
   a connection from redis:connect or redis:connection; a bare port, or
   nothing, is refused loudly."
  (when (nil? conn)
    (error {:error :redis-error
            :reason :no-connection
            :message "no active Redis connection"}))
  (unless (and (struct? conn) (not (nil? (get conn :framer))))
    (error {:error :redis-error
            :reason :not-a-connection
            :where where
            :value conn
            :message (string where ": expected a connection from "
                             "redis:connect or redis:connection")})))

(fn [&named resp]
  (def resp-encode resp:encode)
  (def resp-read resp:read)
  (def resp-read-raw resp:read-raw)
  (def resp-ok? resp:ok?)
  (def redis-connect resp:connect)
  (def redis-close resp:close)

  (defn send-commands [conn & commands]
    "Write commands, each a sequence of arguments, to conn in one
     port/write-many."
    (let [parts @[]]
      (each command in commands
        (each part in (apply resp-encode command)
          (push parts part)))
      (port/write-many conn:port (freeze parts))))

  (defn redis-cmd [& args]
    "Send a command on the current Redis connection and read the reply."
    (let [conn (*redis-conn*)]
      (require-connection conn "redis-cmd")
      (with-redis-lock (fn []
                         (send-commands conn args)
                         (resp-read conn)))))

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
     Returns a struct {:run run :conn-param param}
     where run is a function that takes a thunk and executes it
     with the managed connection bound to param."
    (let [is-terminal (or terminal? default-terminal?)
          retries (or max-retries 3)
          param (parameter nil)]
      (defn run-with-manager [thunk]
        "Execute thunk with a managed Redis connection."
        (def @conn (redis-connect host port))
        (def @attempts 0)
        (defer
          (redis-close conn)
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
                    (let [[close-ok? _] (protect (redis-close conn))])
                    (assign conn (redis-connect host port)))))))))

      {:run run-with-manager :conn-param param}))

  ## ── Client — simplified connection for direct use ─────────────────────

  (defn redis-using [conn thunk]
    "Run thunk with *redis-conn* bound to conn, a connection the caller
     built, and *redis-lock* bound to a new lock. Leaves conn open."
    (require-connection conn "redis-using")
    (parameterize ((*redis-conn* conn)
                   (*redis-lock* (sync:make-lock)))
      (thunk)))

  (defn redis-with [host port thunk]
    "Open a Redis connection, run thunk under redis-using, and close the
     connection on the way out."
    (let [conn (redis-connect host port)]
      (defer
        (redis-close conn)
        (redis-using conn thunk))))

  ## ── Pub/Sub ───────────────────────────────────────────────────────────

  (defn redis-subscribe [conn & channels]
    "Send SUBSCRIBE command on conn. Returns conn (now in sub mode)."
    (send-commands conn (pair "SUBSCRIBE" channels))
    # Read the subscription confirmations
    (each ch in channels
      (resp-read conn))
    conn)

  (defn redis-recv [conn]
    "Read next pub/sub message from a subscribed connection.
     Returns {:channel string :data string} or nil on EOF."
    (let [[ok? msg] (protect (resp-read conn))]
      (if (not ok?)
        nil
        (if (and (array? msg) (>= (length msg) 3) (= (get msg 0) "message"))
          {"channel" (get msg 1) "data" (get msg 2)}
          nil))))

  (defn redis-unsubscribe [conn & channels]
    "Send UNSUBSCRIBE command on conn."
    (send-commands conn (pair "UNSUBSCRIBE" channels))
    # Read the unsubscription confirmations
    (each ch in channels
      (resp-read conn)))

  (defn redis-publish [channel message]
    "PUBLISH channel message — returns number of receivers."
    (redis-cmd "PUBLISH" channel message))

  (defn redis-psubscribe [conn & patterns]
    "Send PSUBSCRIBE command on conn."
    (send-commands conn (pair "PSUBSCRIBE" patterns))
    (each p in patterns
      (resp-read conn))
    conn)

  (defn redis-punsubscribe [conn & patterns]
    "Send PUNSUBSCRIBE command on conn."
    (send-commands conn (pair "PUNSUBSCRIBE" patterns))
    (each p in patterns
      (resp-read conn)))

  ## ── Pipelining ────────────────────────────────────────────────────────

  (defn redis-pipeline [& commands]
    "Send commands, each a sequence like (list \"GET\" \"key\"), in one
     gathered write, and answer an array of their replies. An error reply
     is a struct in the array, from resp-read-raw, so it leaves the replies
     after it in place. The write and the reads hold *redis-lock* when it
     is bound."
    (let [conn (*redis-conn*)]
      (require-connection conn "redis-pipeline")
      (with-redis-lock (fn []
                         (apply send-commands conn commands)
                         (def results @[])
                         (each cmd in commands
                           (push results (resp-read-raw conn)))
                         (freeze results)))))

  {:cmd redis-cmd
   :auth redis-auth
   :manager redis-manager
   :using redis-using
   :with redis-with
   :pipeline redis-pipeline
   :subscribe redis-subscribe
   :recv redis-recv
   :unsubscribe redis-unsubscribe
   :publish redis-publish
   :psubscribe redis-psubscribe
   :punsubscribe redis-punsubscribe})
