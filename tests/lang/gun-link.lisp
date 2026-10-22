(elle/epoch 15)
# audited: 2026-10-21
# std/gun links on the memory store: the soul a chain names, a chain put as a link, and unset.
# lib/gun/chains.md

(include-file "gun/check.lisp")

(def gun ((import "std/gun")))

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
# Souls
# ============================================================================

(let [db (open-db)
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

(let [db (open-db)
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
  (let [other (open-db)]
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

(let [db (open-db)
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
      db (open-db :store store)
      coll (gun:get db "coll")]
  (gun:set coll {:t 1})
  (let [n (writes)]
    (assert (nil? (gun:unset coll (gun:link "stranger") :ack 5))
            "unset of a non-member answers nil, with :ack too")
    (assert (= (writes) n) "and writes nothing")))

(all-passed "gun-link")
