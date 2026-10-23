(elle/epoch 15)
# audited: 2026-10-21
# std/gun admission on load: a user space drops each stored field whose signature does not cover its soul, key, state and value.
# lib/gun/sea.md

(include-file "gun/check.lisp")
(include-file "gun/sea.lisp")

(def crypto (plugin "crypto" (protect (import "plugin/crypto"))))
(def random (plugin "random" (protect (import "plugin/random"))))
(def gun ((import "std/gun") :crypto crypto :random random))
(def value ((import "std/gun/value")))

(def alice (gun:pair (seed-of 1)))
(def alice-root (string "~" alice:pub))

# The fields alice wrote, read back from her store.
(def written
  (let [store (gun:memory-store)
        db (open-db :store store :clock (frozen-clock 1000))]
    (gun:put (gun:get (gun:user db alice) "profile") {:name "Alice" :age 30})
    {:root (store:read alice-root)
     :profile (store:read (string alice-root "/profile"))}))

(defn load-tampered [root profile]
  "Open a database on a store holding root and profile as alice's root and
   profile nodes. Answers [profile-value root-value reasons]."
  (let [store (gun:memory-store)
        errs @[]
        db (open-db :store store :clock (frozen-clock 1000)
                    :on-error (fn [err] (push errs err)))]
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

(all-passed "gun-sea-admission")
