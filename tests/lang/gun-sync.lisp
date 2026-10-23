(elle/epoch 15)
# audited: 2026-10-21
# std/gun peers converging over loopback WebSocket: a clock that runs ahead, states a peer observes, and resync.
# lib/gun/mesh.md
# lib/gun.md

(include-file "gun/check.lisp")
(include-file "gun/loopback.lisp")

(def hash-plugin (plugin "hash" (protect (import "plugin/hash"))))
(def random (plugin "random" (protect (import "plugin/random"))))
(def ws ((import "std/websocket") :hash hash-plugin :random random))
(def gun ((import "std/gun") :websocket ws))

# ============================================================================
# States from the future
# ============================================================================

(defn deferral []
  (let [ahead (gun:clock :now-ms (fn [] (+ (wall-ms) 1500)))
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
  (let [a (open-db :clock (frozen-clock 1000))
        b (open-db :clock (frozen-clock 1000))
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

(deferral)
(observing)
(resync)

(all-passed "gun-sync")
