(elle/epoch 15)
# audited: 2026-10-21
# std/gun user spaces on the memory store: pairs, the signed fields a user chain writes, and the writes a space refuses.
# lib/gun/sea.md

(include-file "gun/check.lisp")
(include-file "gun/sea.lisp")

(def crypto (plugin "crypto" (protect (import "plugin/crypto"))))
(def random (plugin "random" (protect (import "plugin/random"))))
(def gun ((import "std/gun") :crypto crypto :random random))
(def plain-gun ((import "std/gun")))
(def value ((import "std/gun/value")))

(def alice (gun:pair (seed-of 1)))
(def bob (gun:pair (seed-of 2)))
(def alice-root (string "~" alice:pub))

# ============================================================================
# Pairs
# ============================================================================

# A pub taken from the raw seed, rather than from its HKDF derivation, fails
# the first two assertions.
(let [seed (seed-of 1)]
  (assert (= alice:pub
             (seq->hex (crypto:ed25519-public (crypto:hkdf-sha256 seed ""
                       "gun/sea/sign" 32))))
          "pub is the Ed25519 key of the derived signing seed")
  (assert (= alice:epub
             (seq->hex (crypto:x25519-public (crypto:hkdf-sha256 seed ""
                       "gun/sea/encrypt" 32))))
          "epub is the X25519 key of the derived encryption secret")
  (assert (= alice:seed seed) "a pair keeps its seed")
  (assert (= (gun:pair seed) alice) "a seed rebuilds the same pair"))

(assert (not (= alice:pub bob:pub)) "two seeds give two pairs")
(let [fresh (gun:pair)
      other (gun:pair)]
  (assert (= (length fresh:pub) 64) "pub is 64 hex characters")
  (assert (= (length fresh:epub) 64) "epub is 64 hex characters")
  (assert (= (length fresh:seed) 32) "a fresh seed is 32 bytes")
  (assert (not (= fresh:seed other:seed)) "each fresh pair has its own seed"))

(fails-with :bad-seed (fn [] (gun:pair (seed-of 1 31))))
(fails-with :bad-seed (fn [] (gun:pair "not a seed")))

# ============================================================================
# Writing a user space
# ============================================================================

(let [store (gun:memory-store)
      db (open-db :store store :clock (frozen-clock 1000))
      me (gun:user db alice)
      profile (string alice-root "/profile")]
  (gun:put (gun:get me "profile") {:name "Alice"})
  (assert (= (gun:once (gun:get (gun:get db alice-root) "profile"))
             {"name" "Alice"}) "a chain without the pair reads the space")
  (assert (= (gun:once (gun:get me "profile")) {"name" "Alice"})
          "and so does the user chain")
  (let [root (store:read alice-root)
        node (store:read profile)]
    (assert (verifies? alice alice-root "profile" (get root "profile"))
            "the link from the root carries alice's signature")
    (assert (verifies? alice profile "name" (get node "name"))
            "a nested node's field carries it too"))

  (gun:put (gun:get (gun:get me "profile") "name") nil)
  (assert (verifies? alice profile "name" (get (store:read profile) "name"))
          "a tombstone in a space is signed")

  (let [item (gun:set (gun:get me "posts") {:title "hi"})
        posts (gun:once (gun:get me "posts"))]
    (assert (= (length posts) 1) "set adds one member")
    (each [soul _] in posts
      (assert (string/starts-with? soul (string alice-root "/"))
              "a set member in a space gets a soul in the space")
      (assert (verifies? alice soul "title" (get (store:read soul) "title"))
              "and its fields are signed"))
    (assert (= (gun:once (gun:get item "title")) "hi")
            "set answers the new member's chain"))

  # A user chain that follows a link out of every space writes there
  # unsigned, because nothing there needs a signature.
  (gun:put (gun:get db "public") {:note "open"})
  (gun:put (gun:get me "board") (gun:link "public"))
  (gun:put (gun:get (gun:get me "board") "note") "by alice")
  (assert (= (gun:once (gun:get (gun:get db "public") "note")) "by alice")
          "a user chain writes through a link out of its space")
  (assert (= (length (get (store:read "public") "note")) 2)
          "a field outside every space carries no signature"))

# ============================================================================
# Refused writes
# ============================================================================

(let [store (gun:memory-store)
      db (open-db :store store :clock (frozen-clock 1000))
      them (gun:get db alice-root)]
  (gun:put (gun:get (gun:user db alice) "name") "Alice")
  (fails-with :unsigned (fn [] (gun:put (gun:get them "name") "Mallory")))
  (assert (= (gun:once (gun:get them "name")) "Alice")
          "a write into a space without its pair writes nothing")
  (fails-with :unsigned (fn [] (gun:put (gun:get them "fresh") 1)))
  (assert (nil? (get (store:read alice-root) "fresh")) "and stores nothing")

  # Bob links his own field to alice's root, and writes through the link.
  # Bob's pair cannot sign alice's space, so the whole put is refused.
  (let [bob-me (gun:user db bob)]
    (gun:put (gun:get bob-me "friend") (gun:link alice-root))
    (fails-with :unsigned (fn []
                            (gun:put (gun:get bob-me "friend")
                                     {:name "Mallory" :by "bob"})))
    (assert (= (gun:once (gun:get them "name")) "Alice")
            "another pair cannot write the space")))

(let [db (open-db :clock (frozen-clock 1000))]
  (each name in ["~zz" (string "~" (string/uppercase alice:pub))
                 (string "~" alice:pub "0")]
    (fails-with :invalid-space (fn [] (gun:once (gun:get db name))))
    (fails-with :invalid-space (fn [] (gun:put (gun:get db name) {:a 1})))))

(let [db (plain-gun:open)]
  (fails-with :sea-not-configured (fn []
                                    (plain-gun:once (plain-gun:get db alice-root))))
  (fails-with :sea-not-configured (fn []
                                    (plain-gun:put (plain-gun:get db alice-root)
                                    {:a 1})))
  (fails-with :sea-not-configured (fn [] (plain-gun:pair)))
  (fails-with :sea-not-configured (fn [] (plain-gun:user db alice)))
  (fails-with :sea-not-configured (fn [] (plain-gun:secret alice bob:epub)))
  (fails-with :sea-not-configured (fn [] (plain-gun:encrypt (seed-of 0) 1)))
  (fails-with :sea-not-configured (fn []
                                    (plain-gun:decrypt (seed-of 0) (seed-of 0)))))

(all-passed "gun-sea")
