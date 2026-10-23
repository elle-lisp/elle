(elle/epoch 15)
# audited: 2026-10-22
# std/gun reads over loopback WebSocket: a first read that asks the peers, and reads along a chain and around a cycle.
# lib/gun/mesh.md

(include-file "gun/check.lisp")
(include-file "gun/loopback.lisp")

(def hash-plugin (plugin "hash" (protect (import "plugin/hash"))))
(def random (plugin "random" (protect (import "plugin/random"))))
(def ws ((import "std/websocket") :hash hash-plugin :random random))
(def gun ((import "std/gun") :websocket ws))

# ============================================================================
# First reads
# ============================================================================

(defn first-reads []
  (let [a (open-db)
        b (open-db)]
    (gun:put (gun:get a "old") {:v 1})
    (let [srv (serve a)
          hang-up (gun:connect b srv:url)]
      (assert (= (gun:once (gun:get b "old")) {"v" 1})
              "a first read finds what a peer wrote before the connection")
      (hang-up)
      (srv:close)))

  (let [silent (raw-server :silent true)
        d (open-db :wait 0.3)
        hang-up (gun:connect d silent:url)
        @answer :unset
        took (elapsed (fn [] (assign answer (gun:once (gun:get d "nothing")))))]
    (assert (nil? answer) "a peer that never answers yields nothing")
    (assert (>= took 0.3) "a first read waits :wait for a peer that is silent")
    (hang-up)
    (silent:close))

  (let [leaver (raw-server :silent true :leave-on "get")
        d (open-db :wait 60)
        hang-up (gun:connect d leaver:url)]
    (within 10 "a first read whose peer leaves stops waiting for its answer"
            (fn [] (assert (nil? (gun:once (gun:get d "gone"))) "nothing")))
    (hang-up)
    (leaver:close))

  # The second fiber finds the soul pending and waits for the first fetch.
  # A second fetch would send a second get.
  (let [counting (raw-server)
        e (open-db)
        hang-up (gun:connect e counting:url)
        twin (gun:get e "twin")]
    (ev/join [(ev/spawn (fn [] (gun:once twin)))
              (ev/spawn (fn [] (gun:once twin)))])
    (assert (= (length (gets-naming counting:heard "twin")) 1)
            "two first reads of one soul send one get")
    (hang-up)
    (counting:close))

  # B answers A's resync get by asking its other peer, which never answers,
  # so B's fetch of only-a skips A. B's own first read waits for that fetch,
  # and must then ask A. A read that took the skipping fetch as its own
  # answers nil. The sleep lets A's get arrive first; on a box too slow for
  # it, B's read fetches by itself and the test passes without proving this.
  (let [a (open-db)
        b (open-db :wait 1)]
    (gun:put (gun:get a "only-a") {:v 1})
    (let [silent (raw-server :silent true)
          b-to-silent (gun:connect b silent:url)
          srv (serve b)
          a-to-b (gun:connect a srv:url)]
      (ev/sleep 0.2)
      (assert (= (gun:once (gun:get b "only-a")) {"v" 1})
              "a read that waited on a fetch that skipped a peer asks that peer")
      (a-to-b)
      (b-to-silent)
      (srv:close)
      (silent:close)))

  # The same fetch, met by a read on C through B. C's read is answered from
  # B's local data at once, and marks the soul fetched on C, so only a get
  # that B sends A brings A's write to C. Without it, C reads nil for ever.
  (let [a (open-db)
        b (open-db :wait 1)
        c (open-db)]
    (gun:put (gun:get a "relayed") {:v 1})
    (let [silent (raw-server :silent true)
          b-to-silent (gun:connect b silent:url)
          srv-a (serve a)
          srv-b (serve b)
          c-to-b (gun:connect c srv-b:url)
          b-to-a (gun:connect b srv-a:url)]
      (ev/sleep 0.2)
      (gun:once (gun:get c "relayed"))
      (eventually "a read that met a soul pending at B gets A's write as gossip"
                  (fn [] (= (gun:once (gun:get c "relayed")) {"v" 1})))
      (b-to-a)
      (c-to-b)
      (b-to-silent)
      (srv-a:close)
      (srv-b:close)
      (silent:close))))

# ============================================================================
# Forwarding along a chain A - B - C
# ============================================================================

# A holds deep in its store alone, so its resync sends no get for deep, and
# C's read is the one get that reaches B for it. A resync get for deep could
# leave deep pending on B, and C's first read would then answer nil.
(defn chain []
  (let [store-a (gun:memory-store)
        a (open-db :store store-a)
        b (open-db)
        c (open-db)]
    (gun:put (gun:get (gun:open :store store-a) "deep") {:v 1})
    (let [srv-a (serve a)
          srv-b (serve b)
          b-to-a (gun:connect b srv-a:url)
          c-to-b (gun:connect c srv-b:url)]
      (assert (= (gun:once (gun:get c "deep")) {"v" 1})
              "a first read on C finds what only A held, through B")
      (assert (nil? (gun:once (gun:get a "relay"))) "relay starts empty")
      (gun:put (gun:get c "relay") {:v 2} :ack 5)
      (eventually "a put on C reaches A through B"
                  (fn [] (= (gun:once (gun:get a "relay")) {"v" 2})))
      (c-to-b)
      (b-to-a)
      (srv-a:close)
      (srv-b:close))))

# ============================================================================
# A cycle A - B - C - A
# ============================================================================

# Forwarding that ignored the merge result would send the write around the
# cycle without end, and the spy on B would hear it again and again.
(defn cycle []
  (let [a (open-db :wait 20)
        b (open-db :wait 20)
        c (open-db :wait 20)
        srv-a (serve a)
        srv-b (serve b)
        srv-c (serve c)
        hangs [(gun:connect a srv-b:url) (gun:connect b srv-c:url)
               (gun:connect c srv-a:url)]]
    # Without the pending mark, the get would go round the cycle and each
    # hop would wait its 20 seconds for an answer that never comes.
    (within 10 "a first read in a cycle does not wait for its own get"
            (fn [] (assert (nil? (gun:once (gun:get c "nowhere"))) "nothing")))
    (let [spy (raw-client srv-b:url)]
      (gun:put (gun:get a "loop") {:v 1})
      (eventually "B forwards the write to the spy"
                  (fn [] (= (length (puts-of spy:heard "loop" "v")) 1)))
      (ev/sleep 0.5)
      (assert (= (puts-of spy:heard "loop" "v") ["i:1"])
              "the spy hears the write exactly once")
      (spy:close))
    (each h in hangs
      (h))
    (srv-a:close)
    (srv-b:close)
    (srv-c:close)))

(first-reads)
(chain)
(cycle)

(all-passed "gun-mesh-reads")
