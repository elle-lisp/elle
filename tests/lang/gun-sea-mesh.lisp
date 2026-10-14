(elle/epoch 15)
# audited: 2026-10-13
# std/gun user spaces between peers: signed writes travel and verify, and a peer refuses a forged or unsigned put whole.
# lib/gun/sea.md
# lib/gun/mesh.md

## ── Plugin availability check ────────────────────────────────────

(let [[h-ok? _] (protect (import "plugin/hash"))
      [r-ok? _] (protect (import "plugin/random"))
      [c-ok? _] (protect (import "plugin/crypto"))]
  (unless (and h-ok? r-ok? c-ok?)
    (error (struct :error :gated
                   :reason "plugin/hash, plugin/random or plugin/crypto not available"))))

(def crypto (import "plugin/crypto"))
(def ws
  ((import "std/websocket") :hash (import "plugin/hash")
                            :random (import "plugin/random")))
(def gun
  ((import "std/gun") :websocket ws :crypto crypto
                      :random (import "plugin/random")))
(def plain-gun ((import "std/gun") :websocket ws))
(def state ((import "std/gun/state")))
(def value ((import "std/gun/value")))

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

(defn open-db [&named @store @on-error @module]
  (default on-error (fn [err] (push unexpected err)))
  (default module gun)
  (module:open :store store :on-error on-error))

(defn serve [db &opt module]
  "Serve db on a loopback listener, through module, gun by default. Answers
   its :url, and :close."
  (let [listener (tcp/listen "127.0.0.1" 0)
        at (port/path listener)
        m (or module gun)]
    (ev/spawn (fn [] (m:serve db listener)))
    {:url (string "ws://127.0.0.1:"
                  (parse-int (slice at (+ 1 (string/find at ":")))) "/gun")
     :close (fn [] (port/close listener))}))

(defn raw-client [url]
  "A peer by hand that dials url, answers each get with no nodes, and records
   every message it hears. Answers :send, :heard and :close."
  (let [conn (ws:connect url)
        heard @[]
        recv (fn []
               (let [m (ws:recv conn)]
                 (unless (= m:type :close) (json/parse (string m:data)))))]
    (ws:send conn (json/serialize {"hi" "raw-client"}))
    (push heard (recv))
    (let [reader (ev/spawn (fn []
                             (forever
                               (let [msg (recv)]
                                 (when (nil? msg) (break nil))
                                 (push heard msg)
                                 (when (get msg "get")
                                   (ws:send conn
                                   (json/serialize {"ack" (get msg "id")
                                   "put" {}})))))))]
      {:send (fn [msg] (ws:send conn (json/serialize msg)))
       :heard heard
       :close (fn []
                (ev/abort reader)
                (let [t (get conn :transport)]
                  (protect (t:close))))})))

(defn ack-of [heard id]
  "The ack in heard that answers the put id, or nil."
  (find (fn [m] (= (get m "ack") id)) heard))

(defn puts-to [heard soul]
  "The messages in heard whose graph holds soul."
  (filter (fn [m]
            (let [g (get m "put")]
              (and (struct? g) (has-key? g soul)))) heard))

(defn hex->bytes [h]
  (value:decode (string "x:" h)))

(defn sign [seed soul key st v]
  "The hex signature, under the pair of seed, of the field key of soul."
  (seq->hex (crypto:ed25519-sign (crypto:hkdf-sha256 seed "" "gun/sea/sign" 32)
                                 (json/serialize ["gun/sea/sign" soul key st
                                 (value:encode v)]))))

(defn verifies? [pair soul key f]
  "True when field f of key in soul carries the signature of pair."
  (let [[st v sig] f]
    (and (bytes? sig)
         (crypto:ed25519-verify (hex->bytes pair:pub)
                                (json/serialize ["gun/sea/sign" soul key st
                                (value:encode v)]) sig))))

(defn seed-of [n]
  (let [out (@bytes)]
    (each _ in (range 32)
      (push out n))
    (freeze out)))

(def alice-seed (seed-of 1))
(def alice (gun:pair alice-seed))
(def alice-root (string "~" alice:pub))

# ============================================================================
# Signed writes between peers
# ============================================================================

(defn signed-writes []
  (let [b-store (gun:memory-store)
        a (open-db)
        b (open-db :store b-store)
        c (open-db)
        srv-a (serve a)
        srv-b (serve b)
        b-to-a (gun:connect b srv-a:url)
        c-to-b (gun:connect c srv-b:url)
        profile (string alice-root "/profile")]
    (gun:put (gun:get (gun:user a alice) "profile") {:name "Alice"} :ack 5)
    (assert (= (gun:once (gun:get (gun:get b alice-root) "profile"))
               {"name" "Alice"}) "a signed write on A reads on B")
    (assert (verifies? alice profile "name" (get (b-store:read profile) "name"))
            "and reaches B's store with alice's signature")
    (eventually "and reaches C through B"
                (fn []
                  (= (gun:once (gun:get (gun:get c alice-root) "profile"))
                     {"name" "Alice"})))
    (c-to-b)
    (b-to-a)
    (srv-a:close)
    (srv-b:close)))

# ============================================================================
# Refused puts
# ============================================================================

# A raw peer puts into alice's space. Each forged state is after B's own
# write, so HAM would accept the forgery if admission let it through. A spy
# on the same database hears only the put that merged.
(defn refused []
  (let [errs @[]
        b-store (gun:memory-store)
        b (open-db :store b-store :on-error (fn [err] (push errs err)))
        srv (serve b)]
    (gun:put (gun:get (gun:user b alice) "name") "Alice")
    (let [st (get (get (b-store:read alice-root) "name") 0)
          good-sig (sign alice-seed alice-root "name" st "Alice")
          city-sig (sign alice-seed alice-root "city" st "Paris")
          raw (raw-client srv:url)
          spy (raw-client srv:url)
          reasons (fn [] (map (fn [e] (get e :reason)) errs))]
      # Alice's signature over "Alice", on the value "Mallory".
      (raw:send {"id" "forged"
                 "put" {alice-root {"name" [(+ st 1) "s:Mallory" good-sig]}}})
      (eventually "a forged put is acked" (fn [] (ack-of raw:heard "forged")))
      (assert (= (get (ack-of raw:heard "forged") "err") "bad-signature")
              "with err bad-signature")

      (raw:send {"id" "bare" "put" {alice-root {"name" [(+ st 2) "s:Mallory"]}}})
      (eventually "an unsigned put is acked" (fn [] (ack-of raw:heard "bare")))
      (assert (= (get (ack-of raw:heard "bare") "err") "unsigned")
              "with err unsigned")

      # One field is alice's own and new to B; the other is forged. Merging
      # field by field would let the first one in.
      (raw:send {"id" "mixed"
                 "put" {alice-root {"city" [st "s:Paris" city-sig]
                                    "name" [(+ st 3) "s:Mallory" good-sig]}}})
      (eventually "a mixed put is acked" (fn [] (ack-of raw:heard "mixed")))
      (assert (= (get (ack-of raw:heard "mixed") "err") "bad-signature")
              "with err bad-signature")

      (let [space (gun:get b alice-root)]
        (assert (= (gun:once (gun:get space "name")) "Alice")
                "no refused put changes the space")
        (assert (nil? (gun:once (gun:get space "city")))
                "and a valid field in a refused put does not merge"))
      (assert (= (reasons) [:bad-signature :unsigned :bad-signature])
              "each refused put reaches :on-error")

      # The control: the signed city on its own merges and reaches the spy,
      # so the spy's silence before it means something.
      (raw:send {"id" "valid"
                 "put" {alice-root {"city" [st "s:Paris" city-sig]}}})
      (eventually "a valid put is acked" (fn [] (ack-of raw:heard "valid")))
      (assert (= (get (ack-of raw:heard "valid") "ok") true) "with ok true")
      (assert (= (gun:once (gun:get (gun:get b alice-root) "city")) "Paris")
              "a valid signed put from a peer merges")
      (eventually "and is forwarded to the spy"
                  (fn [] (not (empty? (puts-to spy:heard alice-root)))))
      (assert (= (length (puts-to spy:heard alice-root)) 1)
              "the spy heard only the valid put")
      (raw:close)
      (spy:close))
    (srv:close)))

# A database without the crypto plugin cannot check a signature, so it
# refuses the whole put, and the soul outside every space with it.
(defn unconfigured []
  (let [errs @[]
        plain (open-db :module plain-gun :on-error (fn [err] (push errs err)))
        srv (serve plain plain-gun)
        raw (raw-client srv:url)
        st (state:pack (- (wall-ms) 1000) 0)]
    (raw:send {"id" "p1"
               "put" {alice-root {"name" [st "s:Alice"
                                  (sign alice-seed alice-root "name" st "Alice")]}
                      "plain" {"v" [st "i:1"]}}})
    (eventually "the put is acked" (fn [] (ack-of raw:heard "p1")))
    (assert (= (get (ack-of raw:heard "p1") "err") "sea-not-configured")
            "with err sea-not-configured")
    (assert (nil? (plain-gun:once (plain-gun:get plain "plain")))
            "and nothing of it merges")
    (assert (= (map (fn [e] (get e :reason)) errs) [:sea-not-configured])
            "the refusal reaches :on-error")
    (raw:close)
    (srv:close)))

(signed-writes)
(refused)
(unconfigured)

(assert (empty? unexpected)
        (string "no database reported an error: " unexpected))
(println "tests/lang/gun-sea-mesh.lisp: all tests passed")
