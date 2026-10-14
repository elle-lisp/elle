(elle/epoch 15)
# audited: 2026-10-14
# std/gun on the memory store: maps over a node, key ranges, souls, links by chain, and unset.
# lib/gun/chains.md

(def gun ((import "std/gun")))

(defn fails-with [reason thunk]
  "Run thunk, and answer the error it raises after checking its reason."
  (let [[ok? err] (protect (thunk))]
    (assert (not ok?) (string "expected an error with reason " reason))
    (assert (= (get err :error) :gun-error) "a gun error")
    (assert (= (get err :reason) reason) (string "the reason is " reason))
    err))

(defn counting-store []
  "A memory store that counts its writes. Answers [store count], where
   (count) is the number of writes so far."
  (let [inner (gun:memory-store)
        writes (box 0)]
    [{:read (fn [soul] (inner:read soul))
      :write (fn [soul fields]
               (rebox writes (+ (unbox writes) 1))
               (inner:write soul fields))} (fn [] (unbox writes))]))

# ============================================================================
# once on a map
# ============================================================================

(let [db (gun:open)
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

(let [db (gun:open)
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

(let [db (gun:open)
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

(let [db (gun:open)
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

(let [db (gun:open)
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

# ============================================================================
# Souls
# ============================================================================

(let [db (gun:open)
      users (gun:get db "users")
      alice (gun:get users "alice")]
  (fails-with :no-soul (fn [] (gun:soul db)))
  (assert (= (gun:soul users) "users") "a root's soul is its name")
  (assert (= (gun:soul (gun:get db "a/b")) "a%2Fb") "escaped as a root soul is")
  (assert (= (gun:soul alice) "users/alice")
          "a path that holds no link derives its soul")
  (gun:put (gun:get db "carl") {:name "Carl"})
  (gun:put alice {:boss (gun:link "carl")})
  (assert (= (gun:soul (gun:get alice "boss")) "carl")
          "a path whose last key holds a link answers the linked soul")
  (assert (= (gun:soul (gun:get (gun:get alice "boss") "car")) "carl/car")
          "keys after the last link derive from the linked soul")
  (gun:put (gun:get alice "age") 30)
  (assert (= (gun:soul (gun:get alice "age")) "users/alice/age")
          "a field that holds a scalar answers the soul a node put derives")
  (gun:put (gun:get alice "age") {:years 30})
  (assert (= (gun:link-soul (get (gun:once alice) "age")) "users/alice/age")
          "and a node put writes to that soul"))

# ============================================================================
# Links by chain
# ============================================================================

(let [db (gun:open)
      team (gun:get db "team")
      ann (gun:get team "ann")
      bob (gun:get team "bob")]
  (gun:put bob {:name "Bob"})
  (gun:put (gun:get ann "mentor") bob)
  (assert (= (gun:link-soul (get (gun:once ann) "mentor")) "team/bob")
          "a chain put at a field writes a link to the chain's soul")
  (assert (= (gun:once (gun:get (gun:get ann "mentor") "name")) "Bob")
          "and a read follows it")
  (gun:put ann {:name "Ann" :buddy bob})
  (assert (= (gun:link-soul (get (gun:once ann) "buddy")) "team/bob")
          "a chain inside a struct writes a link")
  (assert (= (gun:once bob) {"name" "Bob"})
          "and writes nothing to the chain's own node")
  (gun:put (gun:get ann "later") (gun:get (gun:get db "plans") "next"))
  (assert (= (gun:link-soul (get (gun:once ann) "later")) "plans/next")
          "a chain that names no node yet links to the soul a put would write")
  (let [other (gun:open)]
    (gun:put (gun:get ann "far") (gun:get (gun:get other "x") "y"))
    (assert (= (gun:link-soul (get (gun:once ann) "far")) "x/y")
            "a chain on another database links by its soul")
    (assert (nil? (gun:once (gun:get other "x")))
            "and writes nothing on that database"))
  (fails-with :not-a-node (fn [] (gun:put (gun:get db "root") bob)))
  (fails-with :no-soul (fn [] (gun:put (gun:get ann "self") db)))

  # The chain's trait table holds one protocol, as a link's does.
  (let [t (traits bob)
        methods (get t :Gun)]
    (assert (= (keys t) (list :Gun)) "a chain's table names the :Gun protocol")
    (each [name m] in methods
      (assert (fn? m) (string "and its " name " is a method")))
    (assert (= ((get methods :kind) bob) :chain) ":kind answers :chain")
    (assert (= ((get methods :soul) bob) "team/bob")
            ":soul answers what gun:soul answers")))

# ============================================================================
# Unset
# ============================================================================

(let [db (gun:open)
      coll (gun:get db "coll")
      a (gun:set coll {:t "a"})
      b (gun:set coll {:t "b"})]
  (gun:unset coll a)
  (assert (= (length (gun:once coll)) 1) "unset removes one member")
  (assert (nil? (get (gun:once coll) (gun:soul a)))
          "the member's field is gone from the collection")
  (assert (= (gun:once (gun:get a "t")) "a")
          "and the member's own node is as it was")
  (gun:unset coll (gun:link (gun:soul b)))
  (assert (= (gun:once coll) {}) "unset takes a link as well as a chain")

  # A member that a put stored under keys of its own, twice.
  (gun:put (gun:get db "n1") {:v 1})
  (gun:put coll {:first (gun:link "n1") :again (gun:link "n1") :other 5})
  (gun:unset coll (gun:get db "n1"))
  (assert (= (gun:once coll) {"other" 5})
          "unset removes every field that links to the member, whatever its key")
  (fails-with :no-soul (fn [] (gun:unset db (gun:link "n1")))))

# An unset that finds no member must not write, and must not wait for an ack
# that no write will bring. A put of nothing raises :no-ack at once.
(let [[store writes] (counting-store)
      db (gun:open :store store)
      coll (gun:get db "coll")]
  (gun:set coll {:t 1})
  (let [n (writes)]
    (assert (nil? (gun:unset coll (gun:link "stranger") :ack 5))
            "unset of a non-member answers nil, with :ack too")
    (assert (= (writes) n) "and writes nothing")))

(println "tests/lang/gun-map.lisp: all tests passed")
