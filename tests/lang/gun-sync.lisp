(elle/epoch 14)
# audited: 2026-10-12
# std/gun peers converging over loopback WebSocket: clocks that run ahead, resync, and the walks that fetch.
# lib/gun/mesh.md
# lib/gun.md
#
# Timing checks are lower bounds, so a slow machine passes and a broken rule
# fails.

## ── Plugin availability check ────────────────────────────────────

(let [[h-ok? _] (protect (import "plugin/hash"))
      [r-ok? _] (protect (import "plugin/random"))]
  (unless (and h-ok? r-ok?)
    (error (struct :error :gated
                   :reason "plugin/hash or plugin/random not available"))))

(def ws
  ((import "std/websocket") :hash (import "plugin/hash")
                            :random (import "plugin/random")))
(def gun ((import "std/gun") :websocket ws))
(def state ((import "std/gun/state")))

# ============================================================================
# Helpers
# ============================================================================

(defn wall-ms []
  (integer (* 1000 (clock/realtime))))

(defn eventually [what thunk]
  "Poll thunk until it answers true, for 10 seconds at most."
  (let [deadline (+ (clock/monotonic) 10)]
    (while (and (not (thunk)) (< (clock/monotonic) deadline)) (ev/sleep 0.02))
    (assert (thunk) what)))

# Every database reports here unless a test watches its errors itself. The
# file ends by checking that nothing arrived.
(def unexpected @[])

(defn open-db [&named @clock @wait @store @on-error]
  (default wait 1)
  (default on-error (fn [err] (push unexpected err)))
  (gun:open :clock clock :wait wait :store store :on-error on-error))

(defn listen []
  "A loopback listener on a free port, and the ws:// URL that reaches it."
  (let [listener (tcp/listen "127.0.0.1" 0)
        at (port/path listener)]
    [listener
     (string "ws://127.0.0.1:" (parse-int (slice at (+ 1 (string/find at ":"))))
             "/gun")]))

(defn serve [db]
  "Serve db on a fresh listener. Answers its :url, and :close for the listener."
  (let [[listener url] (listen)]
    (ev/spawn (fn [] (gun:serve db listener)))
    {:url url :close (fn [] (port/close listener))}))

# ── Peers by hand ───────────────────────────────────────────────────────────
# ============================================================================
# States from the future
# ============================================================================

(defn deferral []
  (let [ahead (state:make-clock :now-ms (fn [] (+ (wall-ms) 1500)))
        a (open-db :clock ahead)
        b (open-db)
        srv (serve a)
        hang-up (gun:connect b srv:url)
        seen @[]
        stop (gun:on (gun:get (gun:get b "future") "v")
                     (fn [v] (push seen [v (wall-ms)])))
        t0 (wall-ms)]
    (gun:put (gun:get (gun:get a "future") "v") "ahead" :ack 5)
    # The trap: the merge lands before the subscription is called, and the
    # forward between them yields. Waiting on once instead of on seen reads
    # the value in that gap, and stops the subscription before it is called.
    (eventually "B's subscription sees the future write"
                (fn [] (= (length seen) 2)))
    (stop)
    (let [[v at] (get seen 1)]
      (assert (= v "ahead") "B's subscription saw the write")
      (assert (>= at (+ t0 1500))
              "B held the write until its state was due on B's clock"))
    (hang-up)
    (srv:close)))

# Both clocks stand still at 1000, so A's eighth write carries counter 7.
# B's next tick follows that state only when B observed it; otherwise B
# writes at counter 0, and its own write merges as stale.
(defn observing []
  (let [a (open-db :clock (state:make-clock :now-ms (fn [] 1000)))
        b (open-db :clock (state:make-clock :now-ms (fn [] 1000)))
        srv (serve a)
        hang-up (gun:connect b srv:url)
        on-a (gun:get (gun:get a "tick") "v")
        on-b (gun:get (gun:get b "tick") "v")]
    (assert (nil? (gun:once on-b)) "tick starts empty")
    (each i in (range 7)
      (gun:put on-a i))
    (gun:put on-a 7 :ack 5)
    (assert (= (gun:once on-b) 7) "B holds A's last write")
    (gun:put on-b "b")
    (assert (= (gun:once on-b) "b") "B's next write wins on B")
    (eventually "and on A" (fn [] (= (gun:once on-a) "b")))
    (hang-up)
    (srv:close)))

# ============================================================================
# Resync
# ============================================================================

(defn resync []
  (let [store-a (gun:memory-store)
        a (open-db :store store-a)
        b (open-db)
        srv (serve a)
        hang-up (gun:connect b srv:url)
        shared-a (gun:get a "shared")
        shared-b (gun:get b "shared")]
    (gun:put shared-a {:start 1} :ack 5)
    (assert (= (gun:once shared-b) {"start" 1}) "B holds the soul")
    (hang-up)
    (eventually "A sees B leave" (fn [] (empty? (gun:peers a))))
    (gun:put (gun:get shared-a "a") "from a")
    (gun:put (gun:get shared-b "b") "from b")
    (let [hang-up (gun:connect b srv:url)
          both {"start" 1 "a" "from a" "b" "from b"}]
      (eventually "B pulls A's write" (fn [] (= (gun:once shared-b) both)))
      (eventually "A pulls B's write" (fn [] (= (gun:once shared-a) both)))
      (hang-up))
    (srv:close)
    # A fresh database on A's store has loaded nothing, so it pulls nothing.
    # B's pull must bring A's write out of the store.
    (let [a2 (open-db :store store-a)
          srv2 (serve a2)]
      (gun:put (gun:get (gun:open :store store-a) "shared") {:later "stored"})
      (let [hang-up (gun:connect b srv2:url)]
        (eventually "B pulls a write that A holds only in its store"
                    (fn [] (= (get (gun:once shared-b) "later") "stored")))
        (hang-up))
      (srv2:close))))

# ============================================================================
# Subscriptions and the walks that fetch
# ============================================================================

(defn subscriptions []
  (let [a (open-db)
        b (open-db)
        srv (serve a)
        hang-up (gun:connect b srv:url)
        seen @[]
        stop (gun:on (gun:get (gun:get b "live") "v") (fn [v] (push seen v)))]
    (gun:put (gun:get (gun:get a "live") "v") 1 :ack 5)
    (gun:put (gun:get (gun:get a "live") "v") 2 :ack 5)
    (eventually "B's subscription sees A's writes"
                (fn [] (= (freeze seen) [nil 1 2])))
    (stop)
    (hang-up)
    (srv:close)))

# B's subscription walks to carl only after A links it, and B has never
# fetched carl. A walk that read only local data would answer nil for ever.
(defn notify-fetches []
  (let [a (open-db)
        b (open-db)]
    (gun:put (gun:get a "carl") {:name "Carl"})
    (gun:put (gun:get a "alice") {:name "Alice"})
    (let [srv (serve a)
          hang-up (gun:connect b srv:url)
          seen @[]
          stop (gun:on (gun:get (gun:get (gun:get b "alice") "boss") "name")
                       (fn [v] (push seen v)))]
      (gun:put (gun:get (gun:get a "alice") "boss") (gun:link "carl") :ack 5)
      (eventually "the subscription fetches the newly linked node"
                  (fn [] (= (freeze seen) [nil "Carl"])))
      (stop)
      (hang-up)
      (srv:close))))

# A put that planned on local data alone would write a derived node for
# alice's pet and relink pet to it, away from rex.
(defn put-walks-the-mesh []
  (let [a (open-db)
        b (open-db)]
    (gun:put (gun:get a "rex") {:kind "dog"})
    (gun:put (gun:get (gun:get a "users") "alice") {:pet (gun:link "rex")})
    (let [srv (serve a)
          hang-up (gun:connect b srv:url)]
      (gun:put (gun:get (gun:get (gun:get (gun:get b "users") "alice") "pet")
                        "name") "Rex" :ack 5)
      (eventually "the put writes the node that the mesh links"
                  (fn []
                    (= (gun:once (gun:get a "rex")) {"kind" "dog" "name" "Rex"})))
      (assert (= (gun:link-soul (get (gun:once (gun:get (gun:get a "users")
                                     "alice")) "pet")) "rex")
              "and keeps the link")
      (hang-up)
      (srv:close))))

(deferral)
(observing)
(resync)
(subscriptions)
(notify-fetches)
(put-walks-the-mesh)

(assert (empty? unexpected)
        (string "no database reported an error: " unexpected))
(println "tests/lang/gun-sync.lisp: all tests passed")
