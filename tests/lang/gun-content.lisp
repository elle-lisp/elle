(elle/epoch 15)
# audited: 2026-10-14
# std/gun's content node #: each key is the SHA-256 hex of its value's encoding, and admission refuses any other field.
# lib/gun/sea.md
# lib/gun/chains.md

## ── Plugin availability check ────────────────────────────────────

(let [[c-ok? _] (protect (import "plugin/crypto"))]
  (unless c-ok?
    (error (struct :error :gated :reason "plugin/crypto not available"))))

(def crypto (import "plugin/crypto"))
# No :random: the content node needs :crypto alone.
(def gun ((import "std/gun") :crypto crypto))
(def plain-gun ((import "std/gun")))
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

(defn zeros [n]
  "n zero bytes."
  (let [out (@bytes)]
    (each _ in (range n)
      (push out 0))
    (freeze out)))

(defn hash-key [v]
  "The key that lib/gun/sea.md gives v in the content node."
  (seq->hex (crypto:sha256 (value:encode v))))

# Every database reports here unless a test watches its errors itself. The
# file ends by checking that nothing arrived.
(def unexpected @[])

(defn open-db [&named @store @on-error]
  (default on-error (fn [err] (push unexpected err)))
  (gun:open :store store :on-error on-error))

(def hello-key
  "159051e9a9d711b993a56b707677ffc5cb84e01cbbd587db672c08e69e04a324")
(def nil-key "1f8dc03d51e3ddcc59b608508bba5c34aecac15f1b250390a629a4231ad80a9a")

# ============================================================================
# Keys
# ============================================================================

# The fixed keys are sha256sum of "s:hello" and of "n:". A key taken from
# the raw value rather than its encoding would hash "hello", and miss both.
(assert (= (gun:content-key "hello") hello-key)
        "the key of a string is the hash of its encoding")
(assert (= (gun:content-key nil) nil-key) "and so is the key of nil")

(def every-kind
  [nil true false 0 -7 1.0 -0.0 "" "hello" (bytes) (bytes 0 255)
   (gun:link "users/alice")])

(each v in every-kind
  (assert (= (gun:content-key v) (hash-key v))
          (string "the key of each kind of value is its encoding's hash: " v)))

(fails-with :invalid-value (fn [] (gun:content-key {:a 1})))
(fails-with :invalid-value (fn [] (gun:content-key [1 2])))

# ============================================================================
# Writing and reading
# ============================================================================

# identical? and not =, as in tests/lang/gun.lisp: = would let 1.0 come back
# as 1 and -0.0 as 0.0. A link is read from the node, not followed.
(let [db (open-db)
      content (gun:get db "#")]
  (each v in every-kind
    (let [k (gun:put-content db v)]
      (assert (= k (hash-key v)) (string "put-content answers the key: " v))
      (assert (identical? (get (gun:once content) k) v)
              (string "and the value reads back under it: " v))))
  (assert (= (length (gun:once content)) (- (length every-kind) 1))
          "every value but the tombstone is live"))

(let [db (open-db)]
  (assert (= (gun:put-content (gun:get (gun:get db "elsewhere") "x") "hello")
             hello-key) "put-content takes any chain of the database")
  (assert (= (gun:once (gun:get (gun:get db "#") hello-key)) "hello")
          "and writes into #")
  (assert (nil? (gun:once (gun:get db "elsewhere"))) "and nowhere else")
  (gun:put (gun:get db "#") {(hash-key 42) 42})
  (assert (= (gun:once (gun:get (gun:get db "#") (hash-key 42))) 42)
          "a put under the right key writes")
  (gun:put (gun:get (gun:get db "#") (hash-key "at")) "at")
  (assert (= (gun:once (gun:get (gun:get db "#") (hash-key "at"))) "at")
          "and so does a put at the right field")
  (gun:put (gun:get (gun:get db "#") nil-key) nil)
  (assert (nil? (get (gun:once (gun:get db "#")) nil-key))
          "a tombstone passes at the key of nil")
  (fails-with :invalid-value (fn [] (gun:put-content db {:a 1})))
  # No peer is connected, so no ack can come. The value lands first.
  (fails-with :no-ack (fn [] (gun:put-content db "acked" :ack 0.1)))
  (assert (= (gun:once (gun:get (gun:get db "#") (hash-key "acked"))) "acked")
          "put-content passes :ack through, and writes before it waits"))

# ============================================================================
# Refused writes
# ============================================================================

(let [store (gun:memory-store)
      db (open-db :store store)
      content (gun:get db "#")]
  (gun:put-content db "hello")
  (fails-with :bad-hash (fn [] (gun:put content {"abc" "x"})))
  (fails-with :bad-hash (fn [] (gun:put content {(hash-key "y") "x"})))
  (fails-with :bad-hash (fn []
                          (gun:put content
                                   {(string/uppercase (hash-key "x")) "x"})))
  (fails-with :bad-hash (fn [] (gun:put (gun:get content hello-key) "other")))
  (fails-with :bad-hash (fn [] (gun:put (gun:get content hello-key) nil)))
  # One field is right and one is wrong. Merging field by field would let the
  # first one in.
  (fails-with :bad-hash (fn []
                          (gun:put content {(hash-key "ok") "ok" "zz" "bad"})))
  (assert (= (gun:once content) {hello-key "hello"})
          "no refused put changes the content node")
  (assert (= (keys (store:read "#")) (list hello-key)) "or the store"))

(let [db (open-db)]
  (each name in ["#x" "#/abc" "##"]
    (fails-with :invalid-space (fn [] (gun:once (gun:get db name))))
    (fails-with :invalid-space (fn [] (gun:put (gun:get db name) {:a 1}))))
  # A nested struct needs the soul #/<key>. Its key is no hash either, and
  # the soul's reason is the one that names the mistake.
  (fails-with :invalid-space (fn [] (gun:put (gun:get db "#") {"k" {:a 1}})))
  (fails-with :invalid-space (fn []
                               (gun:put (gun:get (gun:get db "#") "k") {:a 1})))
  (assert (nil? (gun:once (gun:get db "#"))) "and writes nothing"))

# ============================================================================
# Writing the same value again
# ============================================================================

(let [db (open-db)
      field-calls @[]
      node-calls @[]
      k (gun:put-content db "hello")
      stop-field (gun:on (gun:get (gun:get db "#") k)
                         (fn [v] (push field-calls v)))
      stop-node (gun:on (gun:get db "#") (fn [v] (push node-calls v)))]
  (assert (= (gun:put-content db "hello") k) "the same value keeps its key")
  (gun:put (gun:get (gun:get db "#") k) "hello")
  (stop-field)
  (stop-node)
  (assert (= (freeze field-calls) ["hello"])
          "a write of the same value calls no field subscription")
  (assert (= (length node-calls) 1) "and no node subscription"))

# ============================================================================
# Admission on load
# ============================================================================

(defn load-tampered [fields]
  "Open a database on a store whose # node holds fields. Answers
   [content-value reasons]."
  (let [store (gun:memory-store)
        errs @[]
        db (open-db :store store :on-error (fn [err] (push errs err)))]
    (store:write "#" fields)
    [(gun:once (gun:get db "#"))
     (map (fn [e] [(get e :reason) (get e :key)]) errs)]))

(def written
  (let [store (gun:memory-store)
        db (open-db :store store)]
    (gun:put-content db "hello")
    (gun:put-content db 7)
    (store:read "#")))

(let [[c reasons] (load-tampered written)]
  (assert (= c {hello-key "hello" (hash-key 7) 7}) "untouched fields load")
  (assert (empty? reasons) "and report nothing"))

(let [[st _] (get written hello-key)
      [c reasons] (load-tampered (merge written {hello-key [st "Mallory"]}))]
  (assert (= c {(hash-key 7) 7}) "a changed value is dropped on load")
  (assert (= reasons [[:bad-hash hello-key]]) "and reported"))

(let [[c reasons] (load-tampered (merge written {"copy" (get written hello-key)}))]
  (assert (= c {hello-key "hello" (hash-key 7) 7})
          "a field copied to another key is dropped")
  (assert (= reasons [[:bad-hash "copy"]]) "and reported"))

(let [[st v] (get written hello-key)
      sig (zeros 64)
      [c reasons] (load-tampered (merge written {hello-key [st v sig]}))]
  (assert (= c {(hash-key 7) 7}) "a signed field of # is dropped on load")
  (assert (= reasons [[:bad-signature hello-key]])
          "and reported as a bad signature"))

# ============================================================================
# Without :crypto
# ============================================================================

(let [db (plain-gun:open)]
  (fails-with :sea-not-configured (fn [] (plain-gun:once (plain-gun:get db "#"))))
  (fails-with :sea-not-configured (fn []
                                    (plain-gun:put (plain-gun:get db "#")
                                    {hello-key "hello"})))
  (fails-with :sea-not-configured (fn [] (plain-gun:content-key "hello")))
  (fails-with :sea-not-configured (fn [] (plain-gun:put-content db "hello"))))

(assert (empty? unexpected)
        (string "no database reported an error: " unexpected))
(println "tests/lang/gun-content.lisp: all tests passed")
