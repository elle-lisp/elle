(elle/epoch 9)
# Test: struct literal with apply in if-else branch
#
# When a struct literal has a nested struct value ({}) AND the if-else
# branch contains an `apply` (which desugars to CallArrayMut via splice),
# the emitter's DupN for ensure_on_top can create orphan stack entries
# in the else block.  If the orphans are not cleaned up before the
# merge block, the stack depth mismatch causes wrong DupN offsets,
# shifting key-value pairs so a struct lands in a key position.
#
# This is a regression test for the "expected hashable value, got struct"
# bug in struct literal compilation.

# ── minimal reproduction ──────────────────────────────────

(def @bp @[])

# Struct literal with {} value AND apply in else branch
(def result {:headers {} :body (if (empty? bp) nil (apply concat (freeze bp)))})
(assert (= (get result :headers) {}) "headers should be empty struct")
(assert (= (get result :body) nil) "body should be nil for empty body-parts")

# Same with non-empty body-parts (exercises the else branch)
(def @parts @["hello" " " "world"])
(def result2
  {:headers {} :body (if (empty? parts) nil (apply concat (freeze parts)))})
(assert (= (get result2 :headers) {}) "headers preserved with non-empty parts")
(assert (= (get result2 :body) "hello world") "body should be concatenated")

# ── variations ────────────────────────────────────────────

# Multiple struct values before the if-apply
(def result3 {:a {} :b {} :c (if (empty? bp) nil (apply + [1 2]))})
(assert (= (get result3 :a) {}) "first nested struct preserved")
(assert (= (get result3 :b) {}) "second nested struct preserved")
(assert (= (get result3 :c) nil) "if result correct")

# Inside a defn called from different contexts
(defn build-request [body-parts]
  {:method :get
   :path "/test"
   :headers {}
   :body (if (empty? body-parts) nil (apply concat (freeze body-parts)))})

(def r1 (build-request @[]))
(assert (= (get r1 :method) :get) "method from defn")
(assert (= (get r1 :headers) {}) "headers from defn")
(assert (= (get r1 :body) nil) "body nil from defn")

(def r2 (build-request @["a" "b"]))
(assert (= (get r2 :body) "ab") "body concat from defn")

# Inside ev/spawn (original failure context)
(ev/spawn (fn []
            (def r3 (build-request @[]))
            (assert (= (get r3 :headers) {}) "headers in fiber")
            (assert (= (get r3 :body) nil) "body nil in fiber")))
