(elle/epoch 15)
# audited: 2026-10-14
# std/gun on the memory store: states, values, the HAM verdict, souls, merges and the chain API.
# lib/gun.md
#
# The submodules are loaded by hand, in the order lib/gun/overview.md gives,
# so their pure functions are tested without a database around them.

(def gun ((import "std/gun")))
(def state ((import "std/gun/state")))
(def value ((import "std/gun/value")))
(def ham ((import "std/gun/ham") :state state :value value))
(def graph ((import "std/gun/graph") :state state :value value :ham ham))
(def stores ((import "std/gun/store") :state state :value value))

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

# ============================================================================
# States
# ============================================================================

(let [c (frozen-clock 1000)
      a (c:tick)
      b (c:tick)
      d (c:tick)]
  (assert (= a (state:pack 1000 0)) "the first tick is the wall clock")
  (assert (state:before? a b) "a tick follows the tick before it")
  (assert (state:before? b d) "and the one after follows it")
  (assert (= (state:unpack d) [1000 2])
          "a frozen clock counts within its millisecond"))

(let [c (frozen-clock 1000)]
  (c:observe (state:pack 5000 7))
  (assert (= (c:tick) (state:pack 5000 8)) "a tick follows an observed state")
  (c:observe (state:pack 10 0))
  (assert (= (c:tick) (state:pack 5000 9))
          "observing an older state moves nothing"))

(let [@ms 1000
      c (state:make-clock :now-ms (fn [] ms))]
  (c:tick)
  (assign ms 2000)
  (assert (= (c:tick) (state:pack 2000 0))
          "a later wall clock restarts the counter"))

(let [c (frozen-clock 1000)]
  (assert (= (c:now) (- (state:pack 1001 0) 1))
          "now is the last state of the current millisecond")
  (c:observe (state:pack 9000 3))
  (assert (= (c:now) (state:pack 9000 3))
          "now never runs behind an observed state"))

(let [c (frozen-clock 1000)]
  (assert (= (c:due-in (state:pack 1000 9)) 0)
          "a state of this millisecond is due")
  (assert (= (c:due-in (state:pack 900 0)) 0) "a past state is due")
  (assert (= (c:due-in (state:pack 1250 0)) 0.25)
          "a later state is due when the wall clock reaches its millisecond")
  (c:observe (state:pack 3000 0))
  (assert (= (c:due-in (state:pack 2000 0)) 0)
          "a state no later than an observed one is due"))

(assert (= (state:unpack (state:pack 1791766440908 65535)) [1791766440908 65535])
        "pack and unpack round-trip")
(assert (= (state:parse (state:format (state:pack 77 3))) (state:pack 77 3))
        "format and parse round-trip")
(assert (not (state:before? (state:pack 5 0) (state:pack 5 0)))
        "a state is not before itself")
(assert (state:before? nil (state:pack 0 0)) "no state is before every state")
(assert (not (state:before? (state:pack 0 0) nil))
        "and nothing is before no state")

# ============================================================================
# Values
# ============================================================================

# identical? and not =: = makes 1 and 1.0 equal and -0.0 and 0.0 equal, so a
# decoder that turned 1.0 into 1, or -0.0 into 0.0, would pass under =.
(each v in [nil true false 0 -7 42 9223372036854775807 1.0 -0.0 0.1 1e300
            -2.5e-300 "" "a b:c" "émoji 👋🏽" "l:not-a-link" (bytes)
            (bytes 0 255 16) (value:link "users/alice")]
  (assert (value:valid? v) (string "valid: " v))
  (assert (identical? (value:decode (value:encode v)) v)
          (string "round-trips: " v)))

(assert (= (value:encode nil) "n:") "nil encodes as n:")
(assert (= (value:encode true) "t:") "true encodes as t:")
(assert (= (value:encode false) "f:") "false encodes as f:")
(assert (= (value:encode -7) "i:-7") "an integer encodes in decimal")
(assert (= (value:encode 1.5) "d:1.5") "a float encodes in decimal")
(assert (= (value:encode "hi") "s:hi") "a string encodes as itself")
(assert (= (value:encode (bytes 0 255)) "x:00ff") "bytes encode in hex")
(assert (= (value:encode (value:link "a/b")) "l:a/b") "a link encodes its soul")
(assert (not (= (value:encode 1) (value:encode 1.0)))
        "an integer and a float encode apart")

(let [l (value:link "users/alice")]
  (assert (value:link? l) "a link is a link")
  (assert (= (value:link-soul l) "users/alice") "a link names its soul")
  (assert (not (value:link? {:soul "users/alice"}))
          "a struct of the same shape is not a link")
  (assert (not (value:link? "users/alice")) "a soul is not a link"))

# A trait table maps each protocol to a struct of methods (docs/traits.md).
# A tag such as {:gun :link} passes link? and every other test in this file;
# only reading the table itself catches it.
(each l in [(value:link "users/alice") (value:decode "l:users/alice")]
  (let [t (traits l)
        methods (get t :Gun)]
    (assert (= (keys t) (list :Gun)) "a link's table names the :Gun protocol")
    (assert (struct? methods) "the protocol is a struct")
    (each [name m] in methods
      (assert (fn? m) (string "and its " name " is a method")))
    (assert (= ((get methods :kind) l) :link) ":kind answers :link")
    (assert (= ((get methods :soul) l) "users/alice")
            ":soul answers the link's soul")))

(each v in [[1] @[1] (list 1) |1| {:a 1} @{:a 1} (fn [] 1) :keyword 'symbol
            (/ 0.0 0.0) (/ 1.0 0.0) (/ -1.0 0.0) @"mutable" (@bytes 1)]
  (assert (not (value:valid? v)) (string "not a value: " v)))

(each enc in ["" "q:1" "i:" "i:x" "d:nope" "x:0" "x:zz" "t:yes"]
  (let [[ok? err] (protect (value:decode enc))]
    (assert (not ok?) (string "does not decode: " enc))
    (assert (= (get err :reason) :bad-encoding)
            "a bad encoding is :bad-encoding")))

(let [samples [nil true false -1 0 1 2.5 "" "a" "b" (bytes 1) (value:link "s")]]
  (each a in samples
    (each b in samples
      (let [ab (value:before? a b)
            ba (value:before? b a)
            same (= (value:encode a) (value:encode b))]
        (assert (= 1 (+ (if ab 1 0) (if ba 1 0) (if same 1 0)))
                "exactly one of a before b, b before a, or the same encoding")))))

# ============================================================================
# The HAM verdict
# ============================================================================

(assert (= (ham:verdict 10 5 "a" nil nil) :accept) "a first write is accepted")
(assert (= (ham:verdict 10 6 "a" 5 "z") :accept) "a later state is accepted")
(assert (= (ham:verdict 10 4 "z" 5 "a") :stale) "an earlier state is stale")
(assert (= (ham:verdict 10 11 "z" 5 "a") :future) "a state after now is future")
(assert (= (ham:verdict 10 11 "z" nil nil) :future)
        "future even with no current field")
(assert (= (ham:verdict 10 10 "z" nil nil) :accept)
        "a state equal to now is not future")
(assert (= (ham:verdict 10 5 "a" 5 "a") :same) "an equal write is same")

# The tiebreak, in both argument orders. A rule that let the incoming value
# win every tie would accept both of these, and two replicas that saw the
# writes in different orders would keep different values.
(assert (= (ham:verdict 10 5 "b" 5 "a") :accept)
        "on a tie the later encoding wins")
(assert (= (ham:verdict 10 5 "a" 5 "b") :same) "and the earlier encoding loses")
(assert (= (ham:verdict 10 5 nil 5 "a") :same)
        "a tombstone does not win a tie it sorts before")

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

# ============================================================================
# Chains
# ============================================================================

(def db (gun:open :clock (frozen-clock 1000)))
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

# ============================================================================
# Stores
# ============================================================================

(let [s (stores:memory-store)]
  (assert (nil? (s:read "nothing"))
          "a memory store reads nil for an absent soul")
  (s:write "n" {"k" [5 "a"] "l" [5 (value:link "m")]})
  (s:write "n" {"k" [6 nil]})
  (assert (= (s:read "n") {"k" [6 nil] "l" [5 (value:link "m")]})
          "a write replaces the fields it names and keeps the rest"))

(let [s (gun:memory-store)]
  (gun:put (gun:get (gun:open :store s :clock (frozen-clock 1000)) "cfg")
           {:theme "dark"})
  (assert (= (gun:once (gun:get (gun:open :store s :clock (frozen-clock 1000))
                                "cfg")) {"theme" "dark"})
          "a second database on the store sees the first one's writes"))

# A store written by a clock that ran ahead loads without :future, and the
# next local write still wins, because loading observes the stored states.
(let [s (gun:memory-store)]
  (gun:put (gun:get (gun:open :store s :clock (frozen-clock 5000)) "cfg")
           {:theme "dark"})
  (let [behind (gun:open :store s :clock (frozen-clock 1000))
        cfg (gun:get behind "cfg")]
    (assert (= (gun:once cfg) {"theme" "dark"})
            "a store ahead of the clock loads")
    (gun:put cfg {:theme "light"})
    (assert (= (gun:once cfg) {"theme" "light"}) "and the next write wins")))

# Two fibers put one field through a store whose first write sleeps. The
# lock holds the second put until the first write returns, so the store
# receives the field's writes in state order. Without the lock, the second
# put merges and stores its newer write while the first sleeps, and then the
# first, older write lands over it: the store keeps a value that memory
# has already replaced.
(let [inner (gun:memory-store)
      @delays [0.05]
      slow {:read (fn [soul] (inner:read soul))
            :write (fn [soul fields]
                     (unless (empty? delays)
                       (let [d (first delays)]
                         (assign delays (slice delays 1))
                         (ev/sleep d)))
                     (inner:write soul fields))}
      field (gun:get (gun:get (gun:open :store slow) "race") "v")]
  (ev/join [(ev/spawn (fn [] (gun:put field "first")))
            (ev/spawn (fn [] (gun:put field "second")))])
  (assert (= (gun:once (gun:get (gun:get (gun:open :store inner) "race") "v"))
             (gun:once field))
          "a slow store write cannot let an older write land last"))

(assert (= (gun:ham 10 5 "b" 5 "a") :accept) "the module exports the verdict")
(fails-with :redis-not-configured (fn [] (gun:redis-store :prefix "unused")))

(println "tests/lang/gun.lisp: all tests passed")
