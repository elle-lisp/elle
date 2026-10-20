(elle/epoch 15)
# audited: 2026-10-20
## Server commands, transactions and Lua scripting, each one redis-cmd with its reply converted.
## lib/redis/overview.md
##
## Loaded via:
##   (def server ((import "std/redis/server") :connection connection :resp resp))
##
## Exports: {:ping :echo :select :flushdb :dbsize :info
##           :multi :exec :discard :watch :unwatch :atomic
##           :eval :evalsha :script-load :script-exists :script-flush}

(fn [&named connection resp]
  (def redis-cmd connection:cmd)
  (def resp-ok? resp:ok?)

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

  {:ping redis-ping
   :echo redis-echo
   :select redis-select
   :flushdb redis-flushdb
   :dbsize redis-dbsize
   :info redis-info
   :multi redis-multi
   :exec redis-exec
   :discard redis-discard
   :watch redis-watch
   :unwatch redis-unwatch
   :atomic redis-atomic
   :eval redis-eval
   :evalsha redis-evalsha
   :script-load redis-script-load
   :script-exists redis-script-exists
   :script-flush redis-script-flush})
