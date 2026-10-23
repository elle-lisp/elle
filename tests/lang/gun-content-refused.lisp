(elle/epoch 15)
# audited: 2026-10-21
# std/gun's content node # refuses a field under a key that is not its value's hash, on a write, on load, and without :crypto.
# lib/gun/sea.md

(include-file "gun/check.lisp")
(include-file "gun/sea.lisp")

(def crypto (plugin "crypto" (protect (import "plugin/crypto"))))
# No :random: the content node needs :crypto alone.
(def gun ((import "std/gun") :crypto crypto))
(def plain-gun ((import "std/gun")))
(def value ((import "std/gun/value")))

(defn hash-key [v]
  "The key that lib/gun/sea.md gives v in the content node."
  (seq->hex (crypto:sha256 (value:encode v))))

(def hello-key (hash-key "hello"))

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
      [c reasons] (load-tampered (merge written
                                        {hello-key [st v (seed-of 0 64)]}))]
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

(all-passed "gun-content-refused")
