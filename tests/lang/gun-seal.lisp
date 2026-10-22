(elle/epoch 15)
# audited: 2026-10-21
# std/gun encryption: the secret two pairs share, a sealed value that opens only under it, and a note left in a space.
# lib/gun/sea.md

(include-file "gun/check.lisp")
(include-file "gun/sea.lisp")

(def crypto (plugin "crypto" (protect (import "plugin/crypto"))))
(def random (plugin "random" (protect (import "plugin/random"))))
(def gun ((import "std/gun") :crypto crypto :random random))
(def value ((import "std/gun/value")))

(def alice (gun:pair (seed-of 1)))
(def bob (gun:pair (seed-of 2)))

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

  # identical? and not =, as in tests/lang/gun-value.lisp: = would let 1.0
  # come back as 1 and -0.0 as 0.0.
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

# The flow of lib/gun/sea.md: alice publishes her epub in her space and
# leaves a note there for bob, and bob reads both through a chain without her
# pair.
(let [db (open-db :clock (frozen-clock 1000))
      me (gun:user db alice)]
  (gun:put (gun:get me "epub") alice:epub)
  (gun:put (gun:get me "note")
           (gun:encrypt (gun:secret alice bob:epub) "for Bob"))
  (let [space (gun:get db (string "~" alice:pub))
        epub (gun:once (gun:get space "epub"))]
    (assert (= (gun:decrypt (gun:secret bob epub)
                            (gun:once (gun:get space "note"))) "for Bob")
            "a note sealed for bob in alice's space opens for bob")))

(all-passed "gun-seal")
