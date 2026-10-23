(elle/epoch 15)
# audited: 2026-10-21
# std/gun peers over loopback WebSocket: two peers that gossip, and the acks a put waits for.
# lib/gun/mesh.md

(include-file "gun/check.lisp")
(include-file "gun/loopback.lisp")

(def hash-plugin (plugin "hash" (protect (import "plugin/hash"))))
(def random (plugin "random" (protect (import "plugin/random"))))
(def ws ((import "std/websocket") :hash hash-plugin :random random))
(def gun ((import "std/gun") :websocket ws))

# ============================================================================
# Two peers
# ============================================================================

(defn two-peers []
  (let [a (open-db)
        b (open-db)
        srv (serve a)
        hang-up (gun:connect b srv:url)]
    (assert (= (length (gun:peers b)) 1) "the dialer lists the server")
    (assert (= (length (gun:peers a)) 1)
            "the server lists the dialer when connect returns")
    (gun:put (gun:get b "note") {:text "from b"} :ack 5)
    (assert (= (gun:once (gun:get a "note")) {"text" "from b"})
            "a put on the dialer reaches the server")
    # B reads memo first, so the next read is local: only gossip can bring
    # A's write.
    (assert (nil? (gun:once (gun:get b "memo"))) "memo starts empty")
    (gun:put (gun:get a "memo") {:text "from a"} :ack 5)
    (assert (= (gun:once (gun:get b "memo")) {"text" "from a"})
            "a put on the server reaches a dialer that has the soul")
    (gun:put (gun:get (gun:get a "users") "alice") {:pet {:name "Rex"}} :ack 5)
    (let [alice (gun:get (gun:get b "users") "alice")]
      (assert (gun:link? (get (gun:once alice) "pet"))
              "a nested put arrives as a link")
      (assert (= (gun:once (gun:get (gun:get alice "pet") "name")) "Rex")
              "and as the node it links"))
    (hang-up)
    (assert (empty? (gun:peers b)) "closing removes the peer at once")
    (eventually "the server removes the peer" (fn [] (empty? (gun:peers a))))
    (srv:close)))

# ============================================================================
# Acks
# ============================================================================

(defn acks []
  (let [lonely (open-db)]
    (within 30 "a put that reached no peer does not wait for its :ack"
            (fn []
              (fails-with :no-ack (fn []
                                    (gun:put (gun:get lonely "x") {:a 1} :ack 60)))))
    (assert (= (gun:once (gun:get lonely "x")) {"a" 1})
            "the write landed locally all the same"))

  (let [mute (raw-server)
        c (open-db)
        hang-up (gun:connect c mute:url)
        took (elapsed (fn []
                        (fails-with :no-ack (fn []
                                      (gun:put (gun:get c "x") {:a 1} :ack 0.2)))))]
    (assert (>= took 0.2) "a put waits its :ack time for a peer that never acks")
    (hang-up)
    (mute:close))

  (let [leaver (raw-server :leave-on "put")
        c (open-db)
        hang-up (gun:connect c leaver:url)]
    (within 10 "a put whose only peer leaves stops waiting for its ack"
            (fn []
              (fails-with :no-ack (fn []
                                    (gun:put (gun:get c "x") {:a 1} :ack 60)))))
    (hang-up)
    (leaver:close))

  (let [errs @[]
        inner (gun:memory-store)
        broken {:read (fn [soul] (inner:read soul))
                :write (fn [soul fields]
                         (error {:error :store-down
                                 :reason :disk-full
                                 :message "disk full"}))}
        b (open-db :store broken :on-error (fn [err] (push errs err)))
        a (open-db)
        srv (serve b)
        hang-up (gun:connect a srv:url)
        err (fails-with :peer-error (fn []
                                      (gun:put (gun:get a "x") {:a 1} :ack 5)))]
    (assert (= (get err :peer-reason) :disk-full)
            "the ack names the peer's reason")
    (assert (= (length (gun:peers a)) 1) "the peer stays connected")
    (eventually "the failing peer reports its own error"
                (fn [] (= (map (fn [e] (get e :reason)) errs) [:disk-full])))
    (hang-up)
    (srv:close)))

(two-peers)
(acks)

(all-passed "gun-mesh")
