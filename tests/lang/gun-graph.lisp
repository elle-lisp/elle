(elle/epoch 15)
# audited: 2026-10-21
# std/gun's graph: the soul a key path names, merges that commute, the due split, and flattening a struct into nodes.
# lib/gun.md
# lib/gun/chains.md

(include-file "gun/check.lisp")

(def gun ((import "std/gun")))
(def state ((import "std/gun/state")))
(def value ((import "std/gun/value")))
(def ham ((import "std/gun/ham") :state state :value value))
(def graph ((import "std/gun/graph") :state state :value value :ham ham))

# ============================================================================
# Souls
# ============================================================================

(assert (= (graph:root-soul "users") "users") "a plain root soul is its name")
(assert (= (graph:child-soul "users" "alice") "users/alice")
        "a child soul joins with /")
(assert (= (graph:child-soul "x" "a/b") "x/a%2Fb") "a / in a key is escaped")
(assert (= (graph:child-soul "x" "a%b") "x/a%25b") "a % in a key is escaped")
(assert (= (graph:root-soul "a/b") "a%2Fb") "a / in a root name is escaped")
(assert (not (= (graph:child-soul "x" "a/b")
                (graph:child-soul (graph:child-soul "x" "a") "b")))
        "the key a/b and the path a, b name different souls")
(assert (not (= (graph:root-soul "a/b")
                (graph:child-soul (graph:root-soul "a") "b")))
        "the root a/b and the key b of root a name different souls")
(assert (not (= (graph:child-soul "x" "%2F") (graph:child-soul "x" "/")))
        "an escaped key and the key it escapes name different souls")

(assert (= (graph:key-of :age) "age") "a keyword key enters by its name")
(assert (= (graph:key-of "age") "age") "a string key is itself")
(fails-with :invalid-key (fn [] (graph:key-of "_")))
(fails-with :invalid-key (fn [] (graph:key-of 1)))

# ============================================================================
# Merging
# ============================================================================

(let [g (graph:make-graph)]
  (assert (nil? (graph:node g "s")) "an empty graph has no node")
  (assert (= (graph:merge g "s" {"k" [5 "a"] "j" [5 1]} 10)
             {"k" [5 "a"] "j" [5 1]})
          "a first merge answers every field as its diff")
  (assert (= (graph:merge g "s" {"k" [4 "z"] "j" [6 2]} 10) {"j" [6 2]})
          "a merge answers only the fields it accepted")
  (assert (= (graph:merge g "s" {"k" [5 "a"]} 10) {})
          "a repeated write answers nothing")
  (assert (= (graph:node g "s") {"k" [5 "a"] "j" [6 2]})
          "the node holds each winner")
  (fails-with :future-state (fn [] (graph:merge g "s" {"k" [11 "z"]} 10)))
  (assert (= (graph:node g "s") {"k" [5 "a"] "j" [6 2]})
          "a future write changes nothing"))

(let [[due later] (graph:due {"a" [5 "x"] "b" [11 "y"] "c" [10 "z"]} 10)]
  (assert (= due {"a" [5 "x"] "c" [10 "z"]})
          "due holds each field not after now")
  (assert (= later {"b" [11 "y"]}) "later holds each field after now"))
(assert (= (graph:due {} 10) [{} {}]) "no fields split into nothing")

# Two replicas that apply the same writes in opposite orders must hold the
# same node. Arrival order deciding the winner fails this.
(let [writes [{"k" [5 "a"]} {"k" [5 "b"]} {"k" [7 nil] "m" [3 true]}
              {"m" [3 false]} {"k" [6 "c"]}]
      g1 (graph:make-graph)
      g2 (graph:make-graph)]
  (each w in writes
    (graph:merge g1 "s" w 100))
  (each w in (reverse writes)
    (graph:merge g2 "s" w 100))
  (assert (= (graph:node g1 "s") (graph:node g2 "s")) "merges commute")
  (assert (= (graph:node g1 "s") {"k" [7 nil] "m" [3 true]})
          "and keep the HAM winners"))

# ============================================================================
# Flattening
# ============================================================================

(let [no-links (fn [soul key] nil)
      out (graph:flatten "users/alice" {:name "Alice" :pet {:name "Rex"}} 9
                         ["users" "alice"] no-links)]
  (assert (= out
             [["users/alice"
               {"name" [9 "Alice"] "pet" [9 (value:link "users/alice/pet")]}]
              ["users/alice/pet" {"name" [9 "Rex"]}]])
          "a nested struct becomes a linked node, stamped with one state"))

(let [linked (fn [soul key]
               (when (= [soul key] ["users/alice" "pet"]) "pets/rex"))
      out (graph:flatten "users/alice" {:pet {:age 3}} 9 ["users" "alice"]
                         linked)]
  (assert (= out
             [["users/alice" {"pet" [9 (value:link "pets/rex")]}]
              ["pets/rex" {"age" [9 3]}]])
          "a nested struct writes through a link that exists"))

(let [err (fails-with :invalid-value (fn []
                                       (graph:flatten "users/alice"
                                       {:ok 1 :deep {:tags [1 2]}} 9
                                       ["users" "alice"] (fn [s k] nil))))]
  (assert (= (get err :path) ["users" "alice" "deep" "tags"])
          "the error names the key path"))

(all-passed "gun-graph")
