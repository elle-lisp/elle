(elle/epoch 15)
# audited: 2026-10-21
# The loopback harness of the std/gun mesh tests: waits, listeners, and peers by hand that speak the peer messages.
# lib/gun/mesh.md
# tests/lang/overview.md
#
# A file that includes this one includes check.lisp too, and binds gun, loaded
# with :websocket, and ws, the std/websocket module. A wait here is a lower
# bound, or a deadline far below the wait that a broken rule would hit, so a
# slow machine passes and a broken rule fails.

(defn wall-ms []
  "The wall clock in milliseconds."
  (integer (* 1000 (clock/realtime))))

(defn elapsed [thunk]
  "The seconds that thunk takes."
  (let [t0 (clock/monotonic)]
    (thunk)
    (- (clock/monotonic) t0)))

(defn within [seconds what thunk]
  "Run thunk, and fail unless it ends within seconds."
  (assert (ev/timeout seconds
                      (fn []
                        (thunk)
                        true)) what))

(defn until [thunk]
  "Poll thunk until it answers true."
  (while (not (thunk)) (ev/sleep 0.02)))

(defn eventually [what thunk]
  "Poll thunk until it answers true, for 10 seconds at most."
  (within 10 what (fn [] (until thunk))))

# ── Listeners ───────────────────────────────────────────────────────────────

(defn listen [&opt port]
  "A loopback listener on port, or on a free port, and the ws:// URL that
   reaches it. Answers [listener url port]."
  (let [listener (tcp/listen "127.0.0.1" (or port 0))
        at (port/path listener)
        p (parse-int (slice at (+ 1 (string/find at ":"))))]
    [listener (string "ws://127.0.0.1:" p "/gun") p]))

(defn serve-on [db listener &opt module]
  "Serve db through module, gun by default, on listener in a fiber of its
   own. Answers a function that closes the listener."
  (let [m (or module gun)]
    (ev/spawn (fn [] (m:serve db listener)))
    (fn [] (port/close listener))))

(defn serve [db &opt module]
  "Serve db on a fresh listener. Answers its :url, and :close for the listener."
  (let [[listener url _] (listen)]
    {:url url :close (serve-on db listener module)}))

# ── Peers by hand ───────────────────────────────────────────────────────────

(defn raw-send [conn msg]
  "Send msg on conn: text as it is, and a message as JSON."
  (ws:send conn (if (string? msg) msg (json/serialize msg))))

(defn raw-recv [conn]
  "The next message on conn, parsed, or nil when the connection closed."
  (let [m (ws:recv conn)]
    (unless (= m:type :close) (json/parse (string m:data)))))

(defn raw-loop [conn heard silent leave-on]
  "Record each message on conn in heard, and answer each get with no nodes
   unless silent. Return, which closes the connection, when the stream ends
   or on the first message that holds the key leave-on."
  (forever
    (let [msg (raw-recv conn)]
      (when (nil? msg) (break nil))
      (push heard msg)
      (when (and leave-on (get msg leave-on)) (break nil))
      (when (and (not silent) (get msg "get"))
        (raw-send conn {"ack" (get msg "id") "put" {}})))))

(defn raw-server [&named silent leave-on]
  "A peer by hand that serves. It answers each hi, records what it hears in
   :heard, counts the hellos in :hellos, and answers each get with no nodes
   unless :silent. It never acks a put. With :leave-on, it closes a
   connection on the first message that holds that key."
  (let [[listener url _] (listen)
        heard @[]
        hellos (box 0)]
    (ev/spawn (fn []
                (ws:serve listener
                          (fn [conn]
                            (push heard (raw-recv conn))
                            (rebox hellos (+ 1 (unbox hellos)))
                            (raw-send conn {"hi" "raw-server"})
                            (raw-loop conn heard silent leave-on)))))
    {:url url
     :heard heard
     :hellos (fn [] (unbox hellos))
     :close (fn [] (port/close listener))}))

(defn raw-client [url]
  "A peer by hand that dials url, answers each get with no nodes, and records
   what it hears. Answers :send, which takes text or a message, :heard and
   :close."
  (let [conn (ws:connect url)
        heard @[]]
    (raw-send conn {"hi" "raw-client"})
    (push heard (raw-recv conn))
    (let [reader (ev/spawn (fn [] (raw-loop conn heard false nil)))]
      {:send (fn [msg] (raw-send conn msg))
       :heard heard
       :close (fn []
                (ev/abort reader)
                (let [t (get conn :transport)]
                  (protect (t:close))))})))

# ── What a peer by hand heard ───────────────────────────────────────────────

(defn gets-naming [heard soul]
  "The gets in heard that name soul."
  (filter (fn [msg]
            (when-let [souls (get msg "get")]
                      (not (nil? (find (fn [s] (= s soul)) souls))))) heard))

(defn puts-of [heard soul key]
  "The encodings that the puts in heard carry for key of soul."
  (let [out @[]]
    (each msg in heard
      (when-let [g (get msg "put")]
                (when-let [node (get g soul)]
                          (when-let [f (get node key)] (push out (get f 1))))))
    (freeze out)))

(defn puts-to [heard soul]
  "The messages in heard whose graph holds soul."
  (filter (fn [m]
            (let [g (get m "put")]
              (and (struct? g) (has-key? g soul)))) heard))

(defn ack-of [heard id]
  "The ack in heard that answers the put id, or nil."
  (find (fn [m] (= (get m "ack") id)) heard))
