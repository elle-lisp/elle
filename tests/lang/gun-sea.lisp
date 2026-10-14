(elle/epoch 14)
# audited: 2026-10-12
# std/gun user spaces and encryption on the memory store: pairs, signed fields, admission, and sealed values.
# lib/gun/sea.md
# lib/gun.md

## ── Plugin availability check ────────────────────────────────────

(let [[c-ok? _] (protect (import "plugin/crypto"))
      [r-ok? _] (protect (import "plugin/random"))]
  (unless (and c-ok? r-ok?)
    (error (struct :error :gated
                   :reason "plugin/crypto or plugin/random not available"))))

(def crypto (import "plugin/crypto"))
(def random (import "plugin/random"))
(def gun ((import "std/gun") :crypto crypto :random random))
(def plain-gun ((import "std/gun")))
(def state ((import "std/gun/state")))
(def value ((import "std/gun/value")))

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

(defn frozen-clock [ms]
  "A clock whose wall clock stands still at ms."
  (state:make-clock :now-ms (fn [] ms)))

(defn seed-of [n &opt size]
  "A seed of size bytes, 32 by default, each of them n."
  (let [out (@bytes)]
    (each _ in (range (or size 32))
      (push out n))
    (freeze out)))

(defn hex->bytes [h]
  (value:decode (string "x:" h)))

(defn signed-text [soul key st v]
  "The text that lib/gun/sea.md says a signature covers."
  (json/serialize ["gun/sea/sign" soul key st (value:encode v)]))

(defn verifies? [pair soul key f]
  "True when field f of key in soul carries the signature of pair."
  (let [[st v sig] f]
    (and (bytes? sig)
         (crypto:ed25519-verify (hex->bytes pair:pub)
                                (signed-text soul key st v) sig))))

# Every database reports here unless a test watches its errors itself. The
# file ends by checking that nothing arrived.
(def unexpected @[])

(defn open-db [&named @store @on-error]
  (default on-error (fn [err] (push unexpected err)))
  (gun:open :store store :clock (frozen-clock 1000) :on-error on-error))

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
      db (open-db :store store)
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
      db (open-db :store store)
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

(let [db (open-db)]
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

# ============================================================================
# Admission on load
# ============================================================================

# The fields alice wrote, read back from her store.
(def written
  (let [store (gun:memory-store)
        db (open-db :store store)]
    (gun:put (gun:get (gun:user db alice) "profile") {:name "Alice" :age 30})
    {:root (store:read alice-root)
     :profile (store:read (string alice-root "/profile"))}))

(defn load-tampered [root profile]
  "Open a database on a store holding root and profile as alice's root and
   profile nodes. Answers [profile-value root-value reasons]."
  (let [store (gun:memory-store)
        errs @[]
        db (open-db :store store :on-error (fn [err] (push errs err)))]
    (store:write alice-root root)
    (store:write (string alice-root "/profile") profile)
    (let [space (gun:get db alice-root)
          p (gun:once (gun:get space "profile"))
          r (gun:once space)]
      [p r (map (fn [e] [(get e :reason) (get e :key)]) errs)])))

(let [[p r reasons] (load-tampered written:root written:profile)]
  (assert (= p {"name" "Alice" "age" 30}) "untouched fields load")
  (assert (empty? reasons) "and report nothing"))

# The value changes and the signature stays.
(let [[st _ sig] (get written:profile "name")
      [p _ reasons] (load-tampered written:root
                                   (merge written:profile
                                   {"name" [st "Mallory" sig]}))]
  (assert (= p {"age" 30}) "a changed value is dropped on load")
  (assert (= reasons [[:bad-signature "name"]]) "and reported"))

# The state moves later, so a copied field cannot claim a newer write.
(let [[st v sig] (get written:profile "age")
      [p _ reasons] (load-tampered written:root
                                   (merge written:profile
                                   {"age" [(+ st 1) v sig]}))]
  (assert (= p {"name" "Alice"}) "a changed state is dropped on load")
  (assert (= reasons [[:bad-signature "age"]]) "and reported"))

# The signature over name is replayed under another key of the same node.
(let [[p _ reasons] (load-tampered written:root
                                   (merge written:profile
                                   {"nick" (get written:profile "name")}))]
  (assert (= p {"name" "Alice" "age" 30})
          "a field copied to another key is dropped")
  (assert (= reasons [[:bad-signature "nick"]]) "and reported"))

# The same field is replayed into another node of the same space.
(let [[_ r reasons] (load-tampered (merge written:root
                                   {"name" (get written:profile "name")})
                                   written:profile)]
  (assert (= r {"profile" (gun:link (string alice-root "/profile"))})
          "a field copied to another node is dropped")
  (assert (= reasons [[:bad-signature "name"]]) "and reported"))

(let [[st v] (get written:profile "age")
      [p _ reasons] (load-tampered written:root
                                   (merge written:profile {"age" [st v]}))]
  (assert (= p {"name" "Alice"}) "a field with no signature is dropped on load")
  (assert (= reasons [[:unsigned "age"]]) "and reported as unsigned"))

# ============================================================================
# Encryption
# ============================================================================

(let [ab (gun:secret alice bob:epub)
      ba (gun:secret bob alice:epub)
      own (gun:secret alice alice:epub)]
  (assert (= ab ba) "both sides compute one secret")
  (assert (= (length ab) 32) "a secret is 32 bytes")
  (assert (not (= ab own)) "a pair's own secret differs from a shared one")
  (assert (= ab
             (crypto:hkdf-sha256 (crypto:x25519 (crypto:hkdf-sha256 (seed-of 1)
                                 "" "gun/sea/encrypt" 32) (hex->bytes bob:epub))
                                 "" "gun/sea/secret" 32))
          "the secret is the HKDF of the X25519 shared secret")

  # identical? and not =, as in tests/lang/gun.lisp: = would let 1.0 come
  # back as 1 and -0.0 as 0.0.
  (each v in [nil true false 0 -7 1.0 -0.0 "" "for Bob" (bytes) (bytes 0 255)
              (gun:link "users/alice")]
    (let [sealed (gun:encrypt ab v)]
      (assert (bytes? sealed) "a sealed value is bytes")
      (assert (= (length sealed) (+ 24 (length (value:encode v)) 16))
              "a nonce, the sealed encoding, and a tag")
      (assert (identical? (gun:decrypt ba sealed) v)
              (string "decrypt answers what encrypt sealed: " v))))

  (let [sealed (gun:encrypt ab "secret")]
    (assert (not (= sealed (gun:encrypt ab "secret")))
            "two encryptions of one value differ")
    (fails-with :decrypt-failed (fn [] (gun:decrypt own sealed)))
    (let [flipped (thaw sealed)
          last-i (- (length sealed) 1)]
      (put flipped last-i (bit/xor (get flipped last-i) 1))
      (fails-with :decrypt-failed (fn [] (gun:decrypt ab (freeze flipped)))))
    (fails-with :decrypt-failed (fn [] (gun:decrypt ab (slice sealed 0 39))))
    (fails-with :decrypt-failed (fn [] (gun:decrypt ab (bytes))))))

(each epub in ["zz" (string/uppercase bob:epub) (seq->hex (seed-of 0))]
  (fails-with :bad-public-key (fn [] (gun:secret alice epub))))
(fails-with :bad-key (fn [] (gun:encrypt (seed-of 1 31) "x")))
(fails-with :bad-key (fn [] (gun:decrypt (seed-of 1 31) (seed-of 0 64))))

# The flow of lib/gun.md: alice publishes her epub in her space and leaves a
# note there for bob, and bob reads both through a chain without her pair.
(let [db (open-db)
      me (gun:user db alice)]
  (gun:put (gun:get me "epub") alice:epub)
  (gun:put (gun:get me "note")
           (gun:encrypt (gun:secret alice bob:epub) "for Bob"))
  (let [space (gun:get db alice-root)
        epub (gun:once (gun:get space "epub"))]
    (assert (= (gun:decrypt (gun:secret bob epub)
                            (gun:once (gun:get space "note"))) "for Bob")
            "a note sealed for bob in alice's space opens for bob")))

(assert (empty? unexpected)
        (string "no database reported an error: " unexpected))
(println "tests/lang/gun-sea.lisp: all tests passed")
