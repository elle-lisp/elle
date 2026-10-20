(elle/epoch 15)
# audited: 2026-10-20
## The ambient connection: *redis-conn* and the lock beside it, cmd, with, manager, pipeline, and pub/sub over a connection.
## lib/redis/overview.md
##
## Loaded via: (def connection ((import "std/redis/connection") :resp resp))
##
## Concurrency: redis:with binds a mutex alongside the connection.  Every
## redis:cmd and redis:pipeline acquires that mutex around its write+read
## sequence, so callers that share one connection across fibers (e.g.,
## http2:serve spawning a handler fiber per request) cannot interleave
## RESP framing on the wire.  Without this guard, two concurrent fibers
## using a pipeline can each write commands then race on reads — the
## RESP parser then tries to parse the middle of a bulk-string payload
## as the next length prefix.
##
## Exports: {:cmd :auth :manager :with :pipeline
##           :subscribe :recv :unsubscribe :publish :psubscribe :punsubscribe}

(def sync ((import "std/sync")))

## ── Parameter for ambient connection access ───────────────────────────

(def *redis-conn* (parameter nil))

## Mutex paired with *redis-conn*.  Bound by redis-with so concurrent
## fibers sharing one connection cannot interleave writes/reads on
## the wire.  Nil means "no lock needed" — callers that own the
## connection from a single fiber leave the parameter unbound.
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

  (defn redis-cmd [& args]
    "Send a command on the current Redis connection and read the reply."
    (let [conn (*redis-conn*)]
      (require-connection conn "redis-cmd")
      (with-redis-lock (fn []
                         (port/write conn:port (apply resp-encode args))
                         (port/flush conn:port)
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

  (defn redis-with [host port thunk]
    "Open a Redis connection, run thunk with *redis-conn* and *redis-lock*
     bound, close on exit.  The lock serialises wire access across fibers
     sharing the connection — without it, an h2 server that dispatches
     handler fibers under (redis:with ...) sees its concurrent redis-cmd
     / redis-pipeline calls race and corrupt RESP framing."
    (let [conn (redis-connect host port)
          lk (sync:make-lock)]
      (defer
        (redis-close conn)
        (parameterize ((*redis-conn* conn)
                       (*redis-lock* lk))
          (thunk)))))

  ## ── Pub/Sub ───────────────────────────────────────────────────────────

  (defn redis-subscribe [conn & channels]
    "Send SUBSCRIBE command on conn. Returns conn (now in sub mode)."
    (port/write conn:port (apply resp-encode (pair "SUBSCRIBE" channels)))
    (port/flush conn:port)  # Read the subscription confirmations
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
    (port/write conn:port (apply resp-encode (pair "UNSUBSCRIBE" channels)))
    (port/flush conn:port)  # Read the unsubscription confirmations
    (each ch in channels
      (resp-read conn)))

  (defn redis-publish [channel message]
    "PUBLISH channel message — returns number of receivers."
    (redis-cmd "PUBLISH" channel message))

  (defn redis-psubscribe [conn & patterns]
    "Send PSUBSCRIBE command on conn."
    (port/write conn:port (apply resp-encode (pair "PSUBSCRIBE" patterns)))
    (port/flush conn:port)
    (each p in patterns
      (resp-read conn))
    conn)

  (defn redis-punsubscribe [conn & patterns]
    "Send PUNSUBSCRIBE command on conn."
    (port/write conn:port (apply resp-encode (pair "PUNSUBSCRIBE" patterns)))
    (port/flush conn:port)
    (each p in patterns
      (resp-read conn)))

  ## ── Pipelining ────────────────────────────────────────────────────────

  (defn redis-pipeline [& commands]
    "Send multiple commands in a batch, read all replies.
     Each command is a list like (list 'GET' 'key').
     Uses resp-read-raw so error replies don't corrupt state.
     Returns array of results.

     The whole write-flush-read-N sequence is held under *redis-lock*
     (when bound), so concurrent fibers serialise here instead of
     interleaving on the wire — without the lock, fiber A's bulk-string
     payloads can land where fiber B expects its next length prefix."
    (let [conn (*redis-conn*)]
      (require-connection conn "redis-pipeline")
      (with-redis-lock (fn []
                         # Send all commands
                         (each cmd in commands
                           (port/write conn:port (apply resp-encode cmd)))
                         (port/flush conn:port)
                         # Read all replies
                         (def results @[])
                         (each cmd in commands
                           (push results (resp-read-raw conn)))
                         (freeze results)))))

  {:cmd redis-cmd
   :auth redis-auth
   :manager redis-manager
   :with redis-with
   :pipeline redis-pipeline
   :subscribe redis-subscribe
   :recv redis-recv
   :unsubscribe redis-unsubscribe
   :publish redis-publish
   :psubscribe redis-psubscribe
   :punsubscribe redis-punsubscribe})
