(elle/epoch 15)
# audited: 2026-10-21
# std/gun refuses whole a peer's put into a user space that is forged, unsigned, or mixes a valid field with a forged one.
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
(def value ((import "std/gun/value")))

(def alice-seed (seed-of 1))
(def alice (gun:pair alice-seed))
(def alice-root (string "~" alice:pub))

# A raw peer puts into alice's space. Each forged state is after B's own
# write, so HAM would accept the forgery if admission let it through. A spy
# on the same database hears only the put that merged.
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
    (raw:send {"id" "valid" "put" {alice-root {"city" [st "s:Paris" city-sig]}}})
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
  (srv:close))

(all-passed "gun-sea-forged")
