(elle/epoch 15)
# audited: 2026-10-21
# std/gun on the Redis store: the hash layout, signed fields, persistence, interleaved writers, and two peers.
# lib/gun.md
#
# Requires a live Redis on 127.0.0.1:6379, the hash and random plugins for
# the peers, and the crypto plugin for the signed fields. Without any of them,
# the file records a reasoned skip. Every key this file writes starts with
# test:gun:<pid>:, and the file deletes only those keys, so two runs and the
# other Redis tests can share one server.

(include-file "gun/check.lisp")
(include-file "gun/loopback.lisp")
(include-file "gun/sea.lisp")

(def crypto (plugin "crypto" (protect (import "plugin/crypto"))))
(def random (plugin "random" (protect (import "plugin/random"))))
(def hash-plugin (plugin "hash" (protect (import "plugin/hash"))))
(def ws ((import "std/websocket") :hash hash-plugin :random random))
(def redis ((import "std/redis")))
(def gun
  ((import "std/gun") :redis redis :websocket ws :crypto crypto :random random))
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

(defn redis-db [&opt under]
  "A database on the Redis store under prefix, or under prefix:under."
  (open-db :store (gun:redis-store :prefix (if under
                                     (string prefix ":" under)
                                     prefix))))

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
  (let [db (redis-db)
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
  (let [pair (gun:pair (seed-of 3))
        root (string "~" pair:pub)]
    (gun:put (gun:get (gun:user (redis-db) pair) "motto") "a b: c")
    (let [text (get (redis:hgetall (string prefix ":" root)) "motto")
          [st-text sig-hex] (string/split text " ")
          st (state:parse st-text)
          enc (slice text (+ (length st-text) 1 (length sig-hex) 1))]
      (assert (= (length sig-hex) 128) "the second part is the signature in hex")
      (assert (= enc "s:a b: c") "the encoding follows it, spaces and all")
      (assert (crypto:ed25519-verify (hex->bytes pair:pub)
                                     (json/serialize ["gun/sea/sign" root
                                     "motto" st enc]) (hex->bytes sig-hex))
              "the stored signature verifies"))
    # A field that failed admission would reach unexpected.
    (assert (= (gun:once (gun:get (gun:get (redis-db) root) "motto")) "a b: c")
            "a second database loads the signed field")
    (let [s (stores:redis-store :prefix prefix)
          sig (hex->bytes (string/repeat "cd" 64))]
      (s:write "~signed" {"k" [5 "x y" sig] "u" [5 "plain"]})
      (assert (= (s:read "~signed") {"k" [5 "x y" sig] "u" [5 "plain"]})
              "the store reads back a signed and an unsigned field"))))

# ── Persistence ─────────────────────────────────────────────────────────────

(defn persistence []
  (gun:put (gun:get (gun:get (redis-db) "cfg") "theme") "dark")
  (assert (= (gun:once (gun:get (redis-db) "cfg")) {"theme" "dark"})
          "a second database on the prefix sees the first one's write"))

# ── Interleaved writers ─────────────────────────────────────────────────────

# Two fibers write one field through one database, and each write yields on
# its Redis round trips. Afterwards the store must hold what memory holds.
# This reads the end state only, and a later write repairs an earlier one
# that landed out of order, so it passes with the lock removed. The ordering
# itself is pinned in tests/lang/gun-store.lisp, with a store whose first
# write sleeps.
(defn interleaved []
  (let [db (redis-db)
        field (gun:get (gun:get db "race") "v")
        writer (fn [tag]
                 (each i in (range 30)
                   (gun:put field (string tag "-" i))))]
    (ev/join [(ev/spawn (fn [] (writer "a"))) (ev/spawn (fn [] (writer "b")))])
    (let [in-memory (gun:once field)
          fresh (gun:once (gun:get (gun:get (redis-db) "race") "v"))]
      (assert (string? in-memory) "the field holds one of the writes")
      (assert (= fresh in-memory) "the store holds what memory holds")
      (assert (= (get (get (stored "race") "v") 1) (value:encode in-memory))
              "the stored encoding is the winner's"))))

# ── Peers ───────────────────────────────────────────────────────────────────

# The server's reader fibers come from the accept loop, which runs in a fiber
# spawned here. They reach Redis only when they inherit this redis:with.
(defn mesh []
  (let [a (redis-db "a")
        b (redis-db "b")
        srv (serve a)
        hang-up (gun:connect b srv:url)]
    (gun:put (gun:get b "shared") {:v "from b"} :ack 5)
    (assert (= (get (get (stored "a:shared") "v") 1) "s:from b")
            "a write from a peer reaches the server's Redis hash")
    (gun:put (gun:get a "shared") {:w "from a"} :ack 5)
    (assert (= (get (get (stored "b:shared") "w") 1) "s:from a")
            "and a write from the server reaches the dialer's")
    (hang-up)
    (srv:close)))

(gate! (service-up? "127.0.0.1" 6379) "redis not running on 127.0.0.1:6379"
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
                       (mesh)))) (all-passed "gun-redis"))
