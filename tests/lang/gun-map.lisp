(elle/epoch 15)
# audited: 2026-10-21
# std/gun maps on the memory store: once and on over a node's members, key ranges, and a map chain that only reads.
# lib/gun/chains.md

(include-file "gun/check.lisp")

(def gun ((import "std/gun")))

# ============================================================================
# once on a map
# ============================================================================

(let [db (open-db)
      team (gun:get db "team")]
  (gun:put team {:ann {:name "Ann" :age 31} :bob {:name "Bob" :age 27} :size 2})
  (assert (= (gun:once (gun:map team))
             {"ann" {"name" "Ann" "age" 31}
              "bob" {"name" "Bob" "age" 27}
              "size" 2})
          "a member that links is read as its node, and a scalar as itself")
  (assert (= (gun:once (gun:get (gun:map team) "name"))
             {"ann" "Ann" "bob" "Bob"})
          "keys after the map read on from each member, past the scalar")
  (assert (= (gun:once (gun:get (gun:map team) "nothing")) {})
          "a member whose read answers nil is left out")

  (gun:put (gun:get (gun:get team "ann") "pets")
           {:rex {:kind "dog"} :tom {:kind "cat"}})
  (assert (= (gun:once (gun:get (gun:map (gun:get (gun:map team) "pets")) "kind"))
             {"ann" {"rex" "dog" "tom" "cat"}})
          "a map after a map nests one struct in another")

  (gun:put (gun:get db "hollow") {:gone 1})
  (gun:put (gun:get (gun:get db "hollow") "gone") nil)
  (assert (= (gun:once (gun:map (gun:get db "hollow"))) {})
          "a node with no live member answers {}")
  (assert (nil? (gun:once (gun:map (gun:get db "nowhere"))))
          "a chain that reaches no node answers nil")
  (assert (nil? (gun:once (gun:map (gun:get team "size"))))
          "and so does a chain that reaches a scalar")
  (fails-with :no-soul (fn [] (gun:map db))))

# ============================================================================
# Key ranges
# ============================================================================

(let [db (open-db)
      log (gun:get db "log")]
  (gun:put log {"a" 1 "ab" 2 "b" 3 "ba" 4 "c" 5})
  (assert (= (gun:once (gun:map log :prefix "a")) {"a" 1 "ab" 2})
          "a prefix keeps the keys that start with it")
  (assert (= (gun:once (gun:map log :from "ab")) {"ab" 2 "b" 3 "ba" 4 "c" 5})
          ":from keeps its own key")
  (assert (= (gun:once (gun:map log :to "b")) {"a" 1 "ab" 2})
          ":to leaves its own key out")
  (assert (= (gun:once (gun:map log :from "ab" :to "ba")) {"ab" 2 "b" 3})
          "both bounds together")
  (assert (= (gun:once (gun:map log :prefix "b" :from "b" :to "c"))
             {"b" 3 "ba" 4}) "all three together")
  (assert (= (gun:once (gun:map log :prefix "b" :from "ba")) {"ba" 4})
          "a prefix and a bound narrow each other")
  (assert (= (gun:once (gun:map log :from "x")) {})
          "a range that holds no key answers {}")
  (assert (= (+ (length (gun:once (gun:map log :to "b")))
                (length (gun:once (gun:map log :from "b")))) 5)
          "two ranges that meet at one bound share no key and miss none")
  (each bad in [1 :b ["b"]]
    (fails-with :invalid-range (fn [] (gun:map log :prefix bad)))
    (fails-with :invalid-range (fn [] (gun:map log :from bad)))
    (fails-with :invalid-range (fn [] (gun:map log :to bad)))))

# ============================================================================
# on a map
# ============================================================================

(let [db (open-db)
      coll (gun:get db "coll")
      calls @[]
      expect (fn [n call what]
               (assert (= (length calls) n) (string what ": one call"))
               (assert (= (last calls) call) what))]
  (gun:put coll {:a {:n 1}})
  (let [stop (gun:on (gun:get (gun:map coll) "n") (fn [v k] (push calls [k v])))]
    (expect 1 ["a" 1] "on calls its function at once for each member")
    (let [item (gun:set coll {:n 2})
          s (gun:soul item)]
      (expect 2 [s 2] "a member that set adds is called with its value")
      (gun:put (gun:get (gun:get coll "a") "n") 10)
      (expect 3 ["a" 10] "a changed member is called with its new value")
      (gun:put (gun:get (gun:get coll "a") "other") "x")
      (expect 3 ["a" 10] "a write that changes no member's value calls nothing")
      (gun:unset coll item)
      (expect 4 [s nil] "an unset member is called with nil")
      (gun:put (gun:get db "elsewhere") {:m 1})
      (gun:put (gun:get coll "a") (gun:link "elsewhere"))
      (expect 5 ["a" nil]
              "a member relinked to a node without the key is called with nil")
      (gun:put (gun:get (gun:get db "elsewhere") "n") 7)
      (expect 6 ["a" 7] "and is called again when that node gains the key")
      (gun:put (gun:get coll "a") nil)
      (expect 7 ["a" nil] "a tombstoned member is called with nil")
      (stop)
      (gun:set coll {:n 3})
      (expect 7 ["a" nil] "nothing is called after stop"))))

(let [db (open-db)
      coll (gun:get db "ranged")
      calls @[]]
  (gun:put coll {:a1 1 :b1 2})
  (let [stop (gun:on (gun:map coll :prefix "a") (fn [v k] (push calls [k v])))]
    (gun:put (gun:get coll "b2") 3)
    (gun:put (gun:get coll "a2") 4)
    (stop)
    (assert (= (freeze calls) [["a1" 1] ["a2" 4]])
            "on a ranged map calls only for members in the range")))

# ============================================================================
# A map chain only reads
# ============================================================================

(let [db (open-db)
      m (gun:map (gun:get db "x"))]
  (gun:put (gun:get db "x") {:k {:v 1}})
  (fails-with :map-chain (fn [] (gun:put m {:a 1})))
  (fails-with :map-chain (fn [] (gun:put (gun:get m "v") 2)))
  (fails-with :map-chain (fn [] (gun:set m {:a 1})))
  (fails-with :map-chain (fn [] (gun:unset m (gun:link "x/k"))))
  (fails-with :map-chain (fn [] (gun:unset (gun:get db "x") m)))
  (fails-with :map-chain (fn [] (gun:soul m)))
  (fails-with :map-chain (fn [] (gun:put (gun:get db "y") {:ref m})))
  (assert (= (gun:once (gun:get (gun:map (gun:get db "x")) "v")) {"k" 1})
          "a refused write through a map writes nothing")
  (assert (nil? (gun:once (gun:get db "y"))) "nor does a map put as a value"))

(all-passed "gun-map")
