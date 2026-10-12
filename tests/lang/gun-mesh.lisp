(elle/epoch 14)
# audited: 2026-10-12
# std/gun peers over loopback WebSocket: gossip along chains and cycles, first reads, acks, errors and concurrent sends.
# lib/gun/mesh.md
# lib/gun.md
#
# Timing checks are lower bounds, or deadlines far below the wait that a
# broken rule would hit, so a slow machine passes and a broken rule fails.

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

(defn fails-with [reason thunk]
  "Run thunk, and answer the error it raises after checking its reason."
  (let [[ok? err] (protect (thunk))]
    (assert (not ok?) (string "expected an error with reason " reason))
    (assert (= (get err :error) :gun-error) "a gun error")
    (assert (= (get err :reason) reason) (string "the reason is " reason))
    err))

(defn wall-ms []
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

(defn raw-send [conn msg]
  (ws:send conn (json/serialize msg)))

(defn raw-recv [conn]
  "The next message on conn, parsed, or nil when the connection closed."
  (let [m (ws:recv conn)]
    (unless (= m:type :close) (json/parse (string m:data)))))

(defn raw-loop [conn heard answer-gets? &opt leave-on]
  "Record each message on conn. Answer each get with no nodes when answer-gets?.
   Return, which closes the connection, on the first message that holds the
   key leave-on."
  (forever
    (let [msg (raw-recv conn)]
      (when (nil? msg) (break nil))
      (push heard msg)
      (when (and leave-on (get msg leave-on)) (break nil))
      (when (and answer-gets? (get msg "get"))
        (raw-send conn {"ack" (get msg "id") "put" {}})))))

(defn raw-server [answer-gets? &named leave-on]
  "A peer by hand that serves: it answers hi, records what it hears, and
   answers gets only when answer-gets?. It never acks a put. With :leave-on,
   it closes the connection on the first message that holds that key."
  (let [[listener url] (listen)
        heard @[]]
    (ev/spawn (fn []
                (ws:serve listener
                          (fn [conn]
                            (push heard (raw-recv conn))
                            (raw-send conn {"hi" "raw-server"})
                            (raw-loop conn heard answer-gets? leave-on)))))
    {:url url :heard heard :close (fn [] (port/close listener))}))

(defn raw-client [url]
  "A peer by hand that dials url, answers each get with no nodes, and records
   what it hears. Answers :send, :heard and :close."
  (let [conn (ws:connect url)
        heard @[]]
    (raw-send conn {"hi" "raw-client"})
    (push heard (raw-recv conn))
    (let [reader (ev/spawn (fn [] (raw-loop conn heard true)))]
      {:send (fn [text] (ws:send conn text))
       :heard heard
       :close (fn []
                (ev/abort reader)
                (let [t (get conn :transport)]
                  (protect (t:close))))})))

(defn gets-of [heard soul]
  "How many of the gets in heard name soul."
  (length (filter (fn [msg]
                    (let [souls (get msg "get")]
                      (and souls
                           (not (empty? (filter (fn [s] (= s soul)) souls))))))
                  heard)))

(defn puts-of [heard soul key]
  "The encodings that the puts in heard carry for key of soul."
  (let [out @[]]
    (each msg in heard
      (when-let [g (get msg "put")]
                (when-let [node (get g soul)]
                          (when-let [f (get node key)] (push out (get f 1))))))
    (freeze out)))

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

  (let [mute (raw-server true)
        c (open-db)
        hang-up (gun:connect c mute:url)
        took (elapsed (fn []
                        (fails-with :no-ack (fn []
                                      (gun:put (gun:get c "x") {:a 1} :ack 0.2)))))]
    (assert (>= took 0.2) "a put waits its :ack time for a peer that never acks")
    (hang-up)
    (mute:close))

  (let [leaver (raw-server true :leave-on "put")
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

  (let [silent (raw-server false)
        d (open-db :wait 0.3)
        hang-up (gun:connect d silent:url)
        @answer :unset
        took (elapsed (fn [] (assign answer (gun:once (gun:get d "nothing")))))]
    (assert (nil? answer) "a peer that never answers yields nothing")
    (assert (>= took 0.3) "a first read waits :wait for a peer that is silent")
    (hang-up)
    (silent:close))

  (let [leaver (raw-server false :leave-on "get")
        d (open-db :wait 60)
        hang-up (gun:connect d leaver:url)]
    (within 10 "a first read whose peer leaves stops waiting for its answer"
            (fn [] (assert (nil? (gun:once (gun:get d "gone"))) "nothing")))
    (hang-up)
    (leaver:close))

  # The second fiber finds the soul pending and waits for the first fetch.
  # A second fetch would send a second get.
  (let [counting (raw-server true)
        e (open-db)
        hang-up (gun:connect e counting:url)
        twin (gun:get e "twin")]
    (ev/join [(ev/spawn (fn [] (gun:once twin)))
              (ev/spawn (fn [] (gun:once twin)))])
    (assert (= (gets-of counting:heard "twin") 1)
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
    (let [silent (raw-server false)
          b-to-silent (gun:connect b silent:url)
          srv (serve b)
          a-to-b (gun:connect a srv:url)]
      (ev/sleep 0.2)
      (assert (= (gun:once (gun:get b "only-a")) {"v" 1})
              "a read that waited on a fetch that skipped a peer asks that peer")
      (a-to-b)
      (b-to-silent)
      (srv:close)
      (silent:close))))

# ============================================================================
# Forwarding along a chain A - B - C
# ============================================================================

(defn chain []
  (let [a (open-db)
        b (open-db)
        c (open-db)]
    (gun:put (gun:get a "deep") {:v 1})
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

# ============================================================================
# Errors
# ============================================================================

(defn errors []
  (let [errs @[]
        a (open-db)
        b (open-db :on-error (fn [err] (push errs err)))
        srv (serve a)
        hang-up (gun:connect b srv:url)
        field-b (gun:get (gun:get b "boom") "v")
        stop (gun:on field-b
                     (fn [v]
                       (when (= v 1)
                         (error {:error :subscriber
                                 :reason :boom
                                 :message "boom"}))))]
    (gun:put (gun:get (gun:get a "boom") "v") 1 :ack 5)
    (eventually "a subscriber's error in a remote write reaches :on-error"
                (fn [] (= (map (fn [e] (get e :reason)) errs) [:boom])))
    (gun:put (gun:get (gun:get a "boom") "v") 2 :ack 5)
    (assert (= (gun:once field-b) 2) "and the next write still arrives")
    (stop)
    (hang-up)
    (srv:close))

  (let [errs @[]
        a (open-db :on-error (fn [err] (push errs err)))
        srv (serve a)
        raw (raw-client srv:url)
        st (state:pack (- (wall-ms) 1000) 0)]
    (raw:send "not json")
    (raw:send (json/serialize {"id" "r1" "put" {"raw" {"v" [st "s:ok"]}}}))
    (eventually "the valid put is acked"
                (fn []
                  (not (empty? (filter (fn [m] (= (get m "ack") "r1")) raw:heard)))))
    (assert (= (gun:once (gun:get (gun:get a "raw") "v")) "ok")
            "a bad message does not stop the next one")
    (assert (= (map (fn [e] (get e :reason)) errs) [:bad-message])
            "the bad message reaches :on-error")
    (raw:close)
    (srv:close)))

# ============================================================================
# Concurrent sends
# ============================================================================

# The trap: small frames never tear on loopback. The std/websocket transport
# lets two senders interleave only once a write is partial, so the values
# are large enough to fill the socket buffer.
(defn concurrent-sends []
  (let [a (open-db)
        b (open-db)
        srv (serve a)
        hang-up (gun:connect b srv:url)
        on-a (gun:get (gun:get a "burst") "v")
        on-b (gun:get (gun:get b "burst") "v")
        big (string/repeat "x" 300000)
        writer (fn [tag]
                 (each i in (range 8)
                   (gun:put on-a (string tag "-" i big))))]
    (assert (nil? (gun:once on-b)) "burst starts empty")
    (ev/join [(ev/spawn (fn [] (writer "x"))) (ev/spawn (fn [] (writer "y")))
              (ev/spawn (fn [] (writer "z")))])
    (gun:put on-a "final" :ack 20)
    (eventually "B ends on A's last write" (fn [] (= (gun:once on-b) "final")))
    (hang-up)
    (srv:close)))

(two-peers)
(acks)
(first-reads)
(chain)
(cycle)
(errors)
(concurrent-sends)

(assert (empty? unexpected)
        (string "no database reported an error: " unexpected))
(println "tests/lang/gun-mesh.lisp: all tests passed")
