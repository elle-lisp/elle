(elle/epoch 15)
# audited: 2026-11-12
## The write queue of std/framing: each write queues a value where it lies, and a flush sends the queue in one gathered write.
## docs/stdlib/framing.md
##
## std/framing loads this file, and exports its four calls as framing:gather,
## framing:write, framing:flush and framing:join.

(defn gather-open [sink]
  "A gather over sink: a port, which receives each flush as one
   port/write-many, or a function of one argument, which receives each
   flush's parts as an array. The gather never closes its sink."
  (let [send (cond
               (port? sink) (fn [parts] (port/write-many sink parts))
               (fn? sink) sink
               true
                 (error {:error :framing-error
                         :reason :not-a-sink
                         :value sink
                         :message (string
                                    "framing:gather: expected a port or a function, got "
                                    (type-of sink))}))]
    @{:send send :queue @[]}))

(defn gather-write [g value]
  "Queue value, a string or bytes, for the next flush. An immutable value is
   queued as it is; a mutable one is copied once, so the flush sends what it
   held now. An empty value queues nothing. Any other value raises a
   :framing-error whose :reason is :not-bytes."
  (unless (or (string? value) (bytes? value))
    (error {:error :framing-error
            :reason :not-bytes
            :value value
            :message (string "framing:write: expected a string or bytes, got "
                             (type-of value))}))
  (unless (= 0 (length value))
    (push (get g :queue) (if (mutable? value) (freeze value) value)))
  nil)

(defn gather-flush [g]
  "Send every value queued since the last flush, in order, in one write,
   and nothing when the queue is empty. The flush takes the queue and starts
   a new one before it sends, so a write made while the send waits goes out
   with a later flush."
  (let [queued (get g :queue)]
    (unless (empty? queued)
      (put g :queue @[])
      ((get g :send) (freeze queued))))
  nil)

(defn gather-join [parts]
  "The parts of a flush joined into one bytes value, for a sink that copies
   every byte anyway, as TLS does when it encrypts them."
  (def @joined (@bytes))
  (each part in parts
    (append joined (if (bytes? part) part (bytes part))))
  (freeze joined))

## ── Exports ─────────────────────────────────────────────────────────────

(fn []
  {:gather gather-open :write gather-write :flush gather-flush :join gather-join})
