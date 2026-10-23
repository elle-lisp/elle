(elle/epoch 15)
# audited: 2026-10-21
# std/gun's content node #: each key is the SHA-256 hex of its value's encoding, and a write of the same value changes nothing.
# lib/gun/sea.md
# lib/gun/chains.md

(include-file "gun/check.lisp")

(def crypto (plugin "crypto" (protect (import "plugin/crypto"))))
# No :random: the content node needs :crypto alone.
(def gun ((import "std/gun") :crypto crypto))
(def value ((import "std/gun/value")))

(defn hash-key [v]
  "The key that lib/gun/sea.md gives v in the content node."
  (seq->hex (crypto:sha256 (value:encode v))))

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

# identical? and not =, as in tests/lang/gun-value.lisp: = would let 1.0 come
# back as 1 and -0.0 as 0.0. A link is read from the node, not followed.
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

(all-passed "gun-content")
