(elle/epoch 14)
# audited: 2026-10-11
# std/gun on the Redis store: the hash layout, persistence across databases, and interleaved writers.
# lib/gun.md
#
# Requires a live Redis on 127.0.0.1:6379. Every key this file writes starts
# with test:gun:<pid>:, and the file deletes only those keys, so two runs and
# the other Redis tests can share one server.

(def redis ((import "std/redis")))
(def gun ((import "std/gun") :redis redis))
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

# ── Persistence ─────────────────────────────────────────────────────────────

(defn persistence []
  (gun:put (gun:get (gun:get (open-db) "cfg") "theme") "dark")
  (assert (= (gun:once (gun:get (open-db) "cfg")) {"theme" "dark"})
          "a second database on the prefix sees the first one's write"))

# ── Interleaved writers ─────────────────────────────────────────────────────

# Two fibers write one field through one database. Each write yields on its
# Redis round trips, so the fibers interleave. A store that took a field's
# writes out of state order would keep a value older than the one in memory,
# and a fresh database would read the older one.
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

(redis:with "127.0.0.1" 6379
            (fn []
              (clear-test-keys)
              (defer
                (clear-test-keys)
                (store-round-trip)
                (layout)
                (persistence)
                (interleaved))))

(println "tests/lang/gun-redis.lisp: all tests passed")
