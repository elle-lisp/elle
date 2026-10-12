(elle/epoch 14)
# audited: 2026-10-12
# std/gun on the Redis store: the hash layout, signed fields, persistence, interleaved writers, and two peers.
# lib/gun.md
#
# Requires a live Redis on 127.0.0.1:6379, the hash and random plugins for
# the peers, and the crypto plugin for the signed fields. Every key this file
# writes starts with test:gun:<pid>:, and the file deletes only those keys, so
# two runs and the other Redis tests can share one server.

(let [[h-ok? _] (protect (import "plugin/hash"))
      [r-ok? _] (protect (import "plugin/random"))
      [c-ok? _] (protect (import "plugin/crypto"))]
  (unless (and h-ok? r-ok? c-ok?)
    (error (struct :error :gated
                   :reason "plugin/hash, plugin/random or plugin/crypto not available"))))

(def redis ((import "std/redis")))
(def crypto (import "plugin/crypto"))
(def ws
  ((import "std/websocket") :hash (import "plugin/hash")
                            :random (import "plugin/random")))
(def gun
  ((import "std/gun") :redis redis :websocket ws :crypto crypto
                      :random (import "plugin/random")))
(def state ((import "std/gun/state")))
(def value ((import "std/gun/value")))
(def stores ((import "std/gun/store") :state state :value value :redis redis))

(def prefix (string "test:gun:" (sys/pid)))

(defn clear-test-keys []
  (let [keys (redis:scan-all redis:scan :match (string prefix ":*") :count 1000)]
    (when (not (empty? keys)) (apply redis:del keys))))

(defn stored [soul]
  "The Redis hash of soul, each field split into [state encoding]."
  (let [raw (redis:hgetall (string prefix ":" soul))
        out @{}]
    (each [k v] in raw
      (let [space (string/find v " ")]
        (put out k [(state:parse (slice v 0 space)) (slice v (+ space 1))])))
    (freeze out)))

(defn open-db []
  (gun:open :store (gun:redis-store :prefix prefix)))

# ── The store on its own ────────────────────────────────────────────────────

(defn store-round-trip []
  (let [s (stores:redis-store :prefix prefix)
        fields {"nil" [5 nil]
                "t" [5 true]
                "f" [5 false]
                "i" [5 -7]
                "d" [5 0.1]
                "s" [5 "two words: and a colon"]
                "x" [5 (bytes 0 255)]
                "l" [5 (value:link "a/b%2Fc")]}]
    (assert (nil? (s:read "absent")) "an absent soul reads nil")
    (s:write "kinds" fields)
    (let [back (s:read "kinds")]
      (each [k [st v]] in fields
        (let [[bst bv] (get back k)]
          (assert (= bst st) (string "the state of " k " round-trips"))
          (assert (identical? bv v) (string "the value of " k " round-trips")))))
    (s:write "kinds" {"i" [6 8]})
    (assert (= (get (s:read "kinds") "i") [6 8])
            "a write replaces the field it names")
    (assert (= (get (s:read "kinds") "s") [5 "two words: and a colon"])
            "and keeps the others")))

# ── The layout ──────────────────────────────────────────────────────────────

(defn layout []
  (let [db (open-db)
        alice (gun:get (gun:get db "users") "alice")]
    (gun:put (gun:get db "bob") {:name "Bob"})
    (gun:put alice {:name "Alice" :boss (gun:link "bob")})
    (let [node (stored "users/alice")
          [name-state name-enc] (get node "name")
          [boss-state boss-enc] (get node "boss")]
      (assert (= name-enc "s:Alice") "a string field holds its encoding")
      (assert (= boss-enc "l:bob") "a link field holds its soul")
      (assert (= name-state boss-state) "one put stamps one state")
      (assert (= (get (stored "users") "alice") [name-state "l:users/alice"])
              "the parent holds the link to the derived soul"))
    (gun:put (gun:get alice "name") nil)
    (assert (= (get (get (stored "users/alice") "name") 1) "n:")
            "a tombstone is stored")))

# ── Signed fields ───────────────────────────────────────────────────────────

# A field in a user space stores its signature in hex between the state and
# the encoding. The encoding here holds spaces, so a parser that split the
# text at its last space, or that took the hex for part of the encoding,
# reads a different value.
(defn signed-layout []
  (let [seed (let [b (@bytes)]
               (each _ in (range 32)
                 (push b 3))
               (freeze b))
        pair (gun:pair seed)
        root (string "~" pair:pub)
        errs @[]
        db (gun:open :store (gun:redis-store :prefix prefix)
                     :on-error (fn [err] (push errs err)))]
    (gun:put (gun:get (gun:user db pair) "motto") "a b: c")
    (let [text (get (redis:hgetall (string prefix ":" root)) "motto")
          parts (string/split text " ")
          [st-text sig-hex] parts
          st (state:parse st-text)
          enc (slice text (+ (length st-text) 1 (length sig-hex) 1))]
      (assert (= (length sig-hex) 128) "the second part is the signature in hex")
      (assert (= enc "s:a b: c") "the encoding follows it, spaces and all")
      (assert (crypto:ed25519-verify (value:decode (string "x:" pair:pub))
                                     (json/serialize ["gun/sea/sign" root
                                     "motto" st enc])
                                     (value:decode (string "x:" sig-hex)))
              "the stored signature verifies"))
    (let [again (gun:open :store (gun:redis-store :prefix prefix)
                          :on-error (fn [err] (push errs err)))]
      (assert (= (gun:once (gun:get (gun:get again root) "motto")) "a b: c")
              "a second database loads the signed field")
      (assert (empty? errs) (string "no field failed admission: " errs)))
    (let [s (stores:redis-store :prefix prefix)
          sig (value:decode (string "x:" (string/repeat "cd" 64)))]
      (s:write "~signed" {"k" [5 "x y" sig] "u" [5 "plain"]})
      (assert (= (s:read "~signed") {"k" [5 "x y" sig] "u" [5 "plain"]})
              "the store reads back a signed and an unsigned field"))))

# ── Persistence ─────────────────────────────────────────────────────────────

(defn persistence []
  (gun:put (gun:get (gun:get (open-db) "cfg") "theme") "dark")
  (assert (= (gun:once (gun:get (open-db) "cfg")) {"theme" "dark"})
          "a second database on the prefix sees the first one's write"))

# ── Interleaved writers ─────────────────────────────────────────────────────

# Two fibers write one field through one database, and each write yields on
# its Redis round trips. Afterwards the store must hold what memory holds.
# This reads the end state only, and a later write repairs an earlier one
# that landed out of order, so it passes with the lock removed. The ordering
# itself is pinned in tests/lang/gun.lisp, with a store whose first write
# sleeps.
(defn interleaved []
  (let [db (open-db)
        field (gun:get (gun:get db "race") "v")
        writer (fn [tag]
                 (each i in (range 30)
                   (gun:put field (string tag "-" i))))]
    (ev/join [(ev/spawn (fn [] (writer "a"))) (ev/spawn (fn [] (writer "b")))])
    (let [in-memory (gun:once field)
          fresh (gun:once (gun:get (gun:get (open-db) "race") "v"))]
      (assert (string? in-memory) "the field holds one of the writes")
      (assert (= fresh in-memory) "the store holds what memory holds")
      (assert (= (get (get (stored "race") "v") 1) (value:encode in-memory))
              "the stored encoding is the winner's"))))

# ── Peers ───────────────────────────────────────────────────────────────────

# The server's reader fibers come from the accept loop, which runs in a fiber
# spawned here. They reach Redis only when they inherit this redis:with.
(defn mesh []
  (let [errs @[]
        report (fn [err] (push errs err))
        a (gun:open :store (gun:redis-store :prefix (string prefix ":a"))
                    :on-error report)
        b (gun:open :store (gun:redis-store :prefix (string prefix ":b"))
                    :on-error report)
        listener (tcp/listen "127.0.0.1" 0)
        at (port/path listener)
        url (string "ws://127.0.0.1:"
                    (parse-int (slice at (+ 1 (string/find at ":")))) "/gun")]
    (ev/spawn (fn [] (gun:serve a listener)))
    (let [hang-up (gun:connect b url)]
      (gun:put (gun:get b "shared") {:v "from b"} :ack 5)
      (assert (= (get (get (stored "a:shared") "v") 1) "s:from b")
              "a write from a peer reaches the server's Redis hash")
      (gun:put (gun:get a "shared") {:w "from a"} :ack 5)
      (assert (= (get (get (stored "b:shared") "w") 1) "s:from a")
              "and a write from the server reaches the dialer's")
      (hang-up))
    (port/close listener)
    (assert (empty? errs) (string "no database reported an error: " errs))))

(redis:with "127.0.0.1" 6379
            (fn []
              (clear-test-keys)
              (defer
                (clear-test-keys)
                (store-round-trip)
                (layout)
                (signed-layout)
                (persistence)
                (interleaved)
                (mesh))))

(println "tests/lang/gun-redis.lisp: all tests passed")
