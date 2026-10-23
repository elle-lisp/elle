(elle/epoch 15)
# audited: 2026-10-21
# std/gun chains on the memory store: get, put and once, nested nodes, writes through links, sets and subscriptions.
# lib/gun.md
# lib/gun/chains.md

(include-file "gun/check.lisp")

(def gun ((import "std/gun")))

# ============================================================================
# Chains
# ============================================================================

(def db (open-db :clock (frozen-clock 1000)))
(def users (gun:get db "users"))
(def alice (gun:get users "alice"))

(gun:put alice {:name "Alice" :age 30})
(assert (= (gun:once alice) {"name" "Alice" "age" 30}) "put then once")
(assert (= (gun:once (gun:get alice "name")) "Alice")
        "a field chain answers its value")
(assert (= (gun:once (gun:get alice :name)) "Alice")
        "a keyword key names the same field")
(assert (gun:link? (get (gun:once users) "alice")) "the parent holds a link")
(assert (= (gun:link-soul (get (gun:once users) "alice")) "users/alice")
        "the link names the derived soul")
(assert (nil? (gun:once (gun:get db "nobody"))) "an absent node is nil")
(assert (nil? (gun:once (gun:get alice "nothing"))) "an absent field is nil")
(assert (nil? (gun:once (gun:get (gun:get alice "name") "deeper")))
        "a key below a scalar is nil")

(gun:put alice {:pet {:name "Rex"}})
(assert (= (gun:once (gun:get (gun:get alice "pet") "name")) "Rex")
        "a nested put")
(assert (= (gun:once alice)
           {"name" "Alice" "age" 30 "pet" (gun:link "users/alice/pet")})
        "a put merges into the fields already there")

(gun:put (gun:get alice "pet") "none")
(assert (= (gun:once (gun:get alice "pet")) "none") "a scalar replaces a link")
(gun:put (gun:get alice "pet") {:name "Fido"})
(assert (= (gun:once (gun:get (gun:get alice "pet") "name")) "Fido")
        "a struct replaces a scalar with a link")

(gun:put (gun:get db "bob") {:name "Bob"})
(gun:put (gun:get alice "boss") (gun:link "bob"))
(gun:put (gun:get alice "boss") {:title "CEO"})
(assert (= (gun:once (gun:get db "bob")) {"name" "Bob" "title" "CEO"})
        "a put through a link writes the linked node")

(gun:put (gun:get alice "age") nil)
(assert (= (gun:once alice)
           {"name" "Alice"
            "pet" (gun:link "users/alice/pet")
            "boss" (gun:link "bob")}) "a tombstone leaves the struct")
(assert (nil? (gun:once (gun:get alice "age"))) "a tombstoned field reads nil")

(let [before (gun:once alice)
      err (fails-with :invalid-value (fn [] (gun:put alice {:ok 1 :bad [1 2]})))]
  (assert (= (get err :path) ["users" "alice" "bad"])
          "the path starts at the root")
  (assert (= (gun:once alice) before) "a rejected put writes nothing"))
(fails-with :invalid-value (fn [] (gun:put (gun:get alice "x") (/ 0.0 0.0))))
(fails-with :invalid-key (fn [] (gun:put alice {"_" 1})))
(fails-with :not-a-node (fn [] (gun:put (gun:get db "root") 5)))
(fails-with :no-soul (fn [] (gun:put db {:a 1})))
(fails-with :no-soul (fn [] (gun:once db)))

# ============================================================================
# Sets
# ============================================================================

(let [todo (gun:get db "todo")
      a (gun:set todo {:title "one"})
      b (gun:set todo {:title "two"})
      items (gun:once todo)]
  (assert (= (length items) 2) "each set adds one field")
  (each [soul l] in items
    (assert (gun:link? l) "a set member is a link")
    (assert (= (gun:link-soul l) soul) "keyed by its own soul"))
  (assert (= (gun:once (gun:get a "title")) "one")
          "set returns the new node's chain")
  (assert (= (gun:once (gun:get b "title")) "two") "and each node is its own"))

# ============================================================================
# Subscriptions
# ============================================================================

(let [seen @[]
      name (gun:get (gun:get db "carol") "name")
      stop (gun:on name (fn [v] (push seen v)))]
  (assert (= (freeze seen) [nil]) "on fires at once, with nil for nothing")
  (gun:put name "Carol")
  (gun:put name "Carol")
  (gun:put name "Caz")
  (gun:put (gun:get (gun:get db "carol") "age") 40)
  (stop)
  (gun:put name "Carla")
  (assert (= (freeze seen) [nil "Carol" "Caz"])
          "on fires once per change of its own value, and not after stop"))

(let [seen @[]
      carol (gun:get db "carol")
      stop (gun:on carol (fn [v] (push seen v)))]
  (gun:put (gun:get carol "age") 41)
  (stop)
  (assert (= (freeze seen) [{"name" "Carla" "age" 40} {"name" "Carla" "age" 41}])
          "a node subscription sees each change to the node"))

# Relinking a path moves a path subscription to the new node.
(let [seen @[]
      city (gun:get (gun:get (gun:get db "dan") "home") "city")
      stop (gun:on city (fn [v] (push seen v)))]
  (gun:put (gun:get db "paris") {:city "Paris"})
  (gun:put (gun:get db "rome") {:city "Rome"})
  (gun:put (gun:get (gun:get db "dan") "home") (gun:link "paris"))
  (gun:put (gun:get (gun:get db "dan") "home") (gun:link "rome"))
  (stop)
  (assert (= (freeze seen) [nil "Paris" "Rome"])
          "a path subscription follows its links"))

# A function that writes to the database it watches must not deadlock: the
# put that called it has released the database's lock.
(let [echo (gun:get (gun:get db "erin") "echo")
      stop (gun:on (gun:get (gun:get db "erin") "said")
                   (fn [v] (when v (gun:put echo (string "echo " v)))))]
  (gun:put (gun:get (gun:get db "erin") "said") "hi")
  (stop)
  (assert (= (gun:once echo) "echo hi") "a subscriber can write"))

(let [field (gun:get (gun:get db "faye") "x")
      stop (gun:on field
                   (fn [v]
                     (when (= v 2)
                       (error {:error :subscriber :reason :boom :message "boom"}))))]
  (gun:put field 1)
  (let [[ok? err] (protect (gun:put field 2))]
    (assert (not ok?) "a subscriber's error reaches the put")
    (assert (= (get err :reason) :boom) "unchanged"))
  (stop)
  (assert (= (gun:once field) 2) "the write landed before the subscriber ran"))

(all-passed "gun")
