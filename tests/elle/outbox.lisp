(elle/epoch 9)
# Outbox memory management tests
#
# Tests for the outbox-based allocation routing that replaces the
# SharedAllocator. The outbox is a per-yield SlabPool: the child
# fiber allocates yield-bound values into the outbox, the parent reads
# them zero-copy, and the outbox is released on resume.
#
# These tests verify:
# 1. Yield values are readable by the parent (zero-copy)
# 2. Multi-yield generators have bounded memory
# 3. Nested data structures survive yield correctly
# 4. Parent can copy yield values before resume

(def res ((import-file "lib/resource.lisp")))

# ── 1. Yield struct with nested cons ─────────────────────────────

(defn test-yield-nested-struct []
  "Yield a struct containing nested cons cells. Parent reads correctly."
  (let [f (fiber/new (fn []
                       (yield {:name "alice"
                               :scores (cons 90 (cons 85 (cons 92 nil)))
                               :meta {:grade "A"}})) |:yield|)]
    (let [result (fiber/resume f)]
      (assert (= (result :name) "alice") "yield-nested: name field")
      (assert (= (first (result :scores)) 90) "yield-nested: first score")
      (assert (= (first (rest (result :scores))) 85)
              "yield-nested: second score")
      (assert (= (first (rest (rest (result :scores)))) 92)
              "yield-nested: third score")
      (assert (= ((result :meta) :grade) "A")
              "yield-nested: nested struct field")
      (println "1: yield-nested-struct ok"))))

(test-yield-nested-struct)

# ── 2. Multi-yield generator ─────────────────────────────────────

(defn test-multi-yield-generator []
  "Generator yields 1000 structs. Memory should be bounded, not linear."
  (let [m (res:measure (fn []
                         (let [f (fiber/new (fn []
                                 (each i in (range 1000)
                                   (yield {:index i :data (cons i nil)})))
                               |:yield|)]
                           (each _ in (range 1000)
                             (fiber/resume f)))))]
    (assert (< (m :peak) 200)
            (string "multi-yield: peak should be bounded, got " (m :peak)))
    (println (string "2: multi-yield-generator ok (peak=" (m :peak) ")"))))

(test-multi-yield-generator)

# ── 3. Yield value survives across resume ─────────────────────────

(defn test-yield-value-survives []
  "Parent reads yield value, then resumes. Value must be valid before resume."
  (let [f (fiber/new (fn []
                       (yield {:x 42 :y "hello"})
                       (yield {:x 99 :y "world"})) |:yield|)]
    (let [v1 (fiber/resume f)]
      (assert (= (v1 :x) 42) "yield-survives: v1.x before resume")
      (assert (= (v1 :y) "hello") "yield-survives: v1.y before resume")
      (let [saved-x (v1 :x)
            v2 (fiber/resume f)]
        (assert (= saved-x 42) "yield-survives: saved-x after resume")
        (assert (= (v2 :x) 99) "yield-survives: v2.x")
        (assert (= (v2 :y) "world") "yield-survives: v2.y"))))
  (println "3: yield-value-survives ok"))

(test-yield-value-survives)

# ── 4. Private heap unaffected by outbox ──────────────────────────

(defn test-private-heap-intact []
  "Child's private heap survives across yields — local bindings intact."
  (let [f (fiber/new (fn []
                       (let [local-state @[1 2 3]]
                         (yield (length local-state))  # local-state should still be intact after yield
                         (push local-state 4)
                         (yield (length local-state))
                         (push local-state 5)
                         (length local-state))) |:yield|)]
    (assert (= (fiber/resume f) 3) "private-heap: first yield = 3")
    (assert (= (fiber/resume f) 4)
            "private-heap: second yield = 4 (local survived)")
    (assert (= (fiber/resume f) 5) "private-heap: final return = 5"))
  (println "4: private-heap-intact ok"))

(test-private-heap-intact)

# ── 5. Outbox with TCO ───────────────────────────────────────────

(defn test-outbox-tco []
  "TCO loop inside a yielding fiber — rotation + outbox coexist."
  (let [f (fiber/new (fn []
                       (letrec [loop (fn [i acc]
                                       (if (= i 0)
                                         (yield acc)
                                         (loop (- i 1) (+ acc i))))]
                         (loop 1000 0)
                         (letrec [loop2 (fn [i acc]
                                          (if (= i 0)
                                            (yield acc)
                                            (loop2 (- i 1) (+ acc i))))]
                           (loop2 500 0)))) |:yield|)]
    (assert (= (fiber/resume f) 500500) "outbox-tco: first yield = sum(1..1000)")
    (assert (= (fiber/resume f) 125250) "outbox-tco: second yield = sum(1..500)"))
  (println "5: outbox-tco ok"))

(test-outbox-tco)

# ── 6. Resource measurement: outbox scenarios ─────────────────────

(def outbox-scenarios
  [["outbox-yield-struct-1000"
    (fn []
      (let [f (fiber/new (fn []
                           (each i in (range 1000)
                             (yield {:i i :data (cons i nil)}))) |:yield|)]
        (each _ in (range 1000)
          (fiber/resume f))))]

   ["outbox-yield-string-100"
    (fn []
      (let [f (fiber/new (fn []
                           (each i in (range 100)
                             (yield (string "item-" i)))) |:yield|)]
        (each _ in (range 100)
          (fiber/resume f))))]

   ["outbox-yield-cons-chain"
    (fn []
      (let [f (fiber/new (fn []
                           (each i in (range 100)
                             (yield (cons i (cons (+ i 1) (cons (+ i 2) nil))))))
                         |:yield|)]
        (each _ in (range 100)
          (fiber/resume f))))]])

(println "# outbox resource measurements")
(def outbox-results (res:suite outbox-scenarios))

# Memory assertions: outbox values are freed on resume, so peak is bounded
(defn find-outbox-result [name]
  (letrec [loop (fn [i]
                  (if (>= i (length outbox-results))
                    nil
                    (let [entry (outbox-results i)]
                      (if (= (entry 0) name) (entry 1) (loop (+ i 1))))))]
    (loop 0)))

(let [m (find-outbox-result "outbox-yield-struct-1000")]
  (assert (< (m :peak) 200)
          (string "outbox-yield-struct-1000: peak bounded, got " (m :peak))))

(println "6: outbox resource assertions ok")
(println "all outbox tests passed")
