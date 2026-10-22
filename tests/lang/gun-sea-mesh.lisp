(elle/epoch 15)
# audited: 2026-10-21
# std/gun SEA between peers: a signed write verifies on each peer, and a peer refuses whole a put it cannot admit.
# lib/gun/sea.md
# lib/gun/mesh.md

(include-file "gun/check.lisp")
(include-file "gun/loopback.lisp")
(include-file "gun/sea.lisp")

(def crypto (plugin "crypto" (protect (import "plugin/crypto"))))
(def random (plugin "random" (protect (import "plugin/random"))))
(def hash-plugin (plugin "hash" (protect (import "plugin/hash"))))
(def ws ((import "std/websocket") :hash hash-plugin :random random))
(def gun ((import "std/gun") :websocket ws :crypto crypto :random random))
(def plain-gun ((import "std/gun") :websocket ws))
(def state ((import "std/gun/state")))
(def value ((import "std/gun/value")))

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
# Puts a peer cannot admit
# ============================================================================

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

# A raw peer puts "s:other" under the key of "s:hello" in the content node.
# The control after it puts the right value under the same key, so the
# refusal is about the hash and not the soul.
(defn content-refused []
  (let [errs @[]
        b (open-db :on-error (fn [err] (push errs err)))
        srv (serve b)
        raw (raw-client srv:url)
        st (state:pack (- (wall-ms) 1000) 0)
        k (seq->hex (crypto:sha256 "s:hello"))]
    (raw:send {"id" "wrong" "put" {"#" {k [st "s:other"]}}})
    (eventually "a put under a wrong key is acked"
                (fn [] (ack-of raw:heard "wrong")))
    (assert (= (get (ack-of raw:heard "wrong") "err") "bad-hash")
            "with err bad-hash")
    (assert (nil? (gun:once (gun:get b "#"))) "and nothing of it merges")
    (assert (= (map (fn [e] (get e :reason)) errs) [:bad-hash])
            "the refusal reaches :on-error")
    (raw:send {"id" "right" "put" {"#" {k [st "s:hello"]}}})
    (eventually "a put under the right key is acked"
                (fn [] (ack-of raw:heard "right")))
    (assert (= (get (ack-of raw:heard "right") "ok") true) "with ok true")
    (assert (= (gun:once (gun:get (gun:get b "#") k)) "hello") "and merges")
    (raw:close)
    (srv:close)))

(signed-writes)
(unconfigured)
(content-refused)

(all-passed "gun-sea-mesh")
