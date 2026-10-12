(elle/epoch 14)
# audited: 2026-10-11
## The memory store and the Redis store, behind one interface of :read and :write.
## lib/gun.md
## lib/gun/overview.md
##
## Loaded via:
##   (def store ((import "std/gun/store") :state state :value value))
##   (def store ((import "std/gun/store") :state state :value value
##                                        :redis ((import "std/redis"))))
##
## Exports: {:memory-store :redis-store}
##
## :read takes a soul and answers its fields, a struct of key to
## [state value], or nil. :write takes a soul and fields, and replaces the
## fields it names.

(fn [&named state value redis]

  (defn make-store [read write]
    "A store from its two functions."
    {:read read :write write})

  ## ── Memory ───────────────────────────────────────────────────────────

  (defn memory-store []
    "A store that keeps nodes in this process."
    (def nodes @{})
    (make-store (fn [soul] (when-let [n (get nodes soul)] (freeze n)))
                (fn [soul fields]
                  (let [n (or (get nodes soul)
                              (let [fresh @{}]
                                (put nodes soul fresh)
                                fresh))]
                    (each [key f] in fields
                      (put n key f)))
                  nil)))

  ## ── Redis ────────────────────────────────────────────────────────────

  (defn field->text [[st v]]
    "The Redis text of a field: the state, a space, and the encoding."
    (string (state:format st) " " (value:encode v)))

  (defn text->field [text]
    "The [state value] that field->text wrote as text."
    (let [space (string/find text " ")]
      (when (nil? space)
        (error {:error :gun-error
                :reason :bad-encoding
                :encoding text
                :message (string "not a stored field: " text)}))
      [(state:parse (slice text 0 space))
       (value:decode (slice text (+ space 1)))]))

  (defn redis-store [&named @prefix]
    "A store that keeps each node in the Redis hash <prefix>:<soul>, through
     the connection of the caller's redis:with. :prefix defaults to gun.
     Raises :redis-not-configured when this module has no :redis."
    (when (nil? redis)
      (error {:error :gun-error
              :reason :redis-not-configured
              :message ":redis was not supplied to (import \"std/gun\")"}))
    (default prefix "gun")
    (defn hash-key [soul]
      (string prefix ":" soul))
    (make-store (fn [soul]
                  (let [raw (redis:hgetall (hash-key soul))]
                    (unless (empty? raw)
                      (let [out @{}]
                        (each [key text] in raw
                          (put out key (text->field text)))
                        (freeze out)))))
                (fn [soul fields]
                  (unless (empty? fields)
                    (let [args @[]]
                      (each [key f] in fields
                        (push args key)
                        (push args (field->text f)))
                      (apply redis:hmset (hash-key soul) (freeze args))))
                  nil)))

  {:memory-store memory-store :redis-store redis-store})
