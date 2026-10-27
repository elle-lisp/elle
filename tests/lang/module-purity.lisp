(elle/epoch 15)
# audited: 2026-10-26
# A module's value holds no mutable state: fn/captures shows each captured slot, and the loader refuses what code can still change.
# docs/modules.md

(defn refused? [form]
  "Whether import/load-syntax refuses the value of form as a module's."
  (let [[ok? err] (protect (import/load-syntax form))]
    (and (not ok?) (= :module-error (get err :error)))))

(defn mutable-slots [f]
  "How many of f's captured slots code can still assign."
  (length (filter (fn [s] (get s :mutable?)) (fn/captures f))))

# ── fn/captures answers each captured slot ──────────────────────────────

# The counter-factual answered a count, which says nothing about a slot.
(let [sum-of-two (let [a 1
                       b 2]
                   (fn [] (+ a b)))
      slots (fn/captures sum-of-two)]
  (assert (= 2 (length slots)) "one slot per captured binding")
  (assert (= [1 2] (map (fn [s] (get s :value)) slots))
          "each slot holds its value")
  (assert (= 0 (mutable-slots sum-of-two)) "no code assigns a or b"))

(let [counter (let [@n 0
                    k 5]
                (fn []
                  (assign n (+ n k))
                  n))
      slots (fn/captures counter)
      n-slot (find (fn [s] (get s :mutable?)) slots)]
  (assert (= 1 (mutable-slots counter)) "code can still assign n, and not k")
  (assert (= "n" (get n-slot :name))
          "a slot the compiler recorded names its binding")
  (assert (= 0 (get n-slot :value))
          "and holds the binding's value, not its cell")
  (counter)
  (assert (= 5
             (get (find (fn [s] (get s :mutable?)) (fn/captures counter)) :value))
          "the value is read when fn/captures runs"))

(let [unassigned (let [@n 0]
                   (fn [] n))]
  (assert (= 0 (mutable-slots unassigned))
          "a binding declared mutable that no code assigns cannot change"))

(let [f (letrec [even (fn [n] (if (= n 0) true (odd (- n 1))))
                 odd (fn [n] (if (= n 0) false (even (- n 1))))]
          even)]
  (assert (= 0 (mutable-slots f))
          "a letrec binding a closure captures is not assigned by any code"))

(assert (nil? (fn/captures clock/monotonic)) "a native primitive has no slots")
(assert (nil? (fn/captures 42)) "nor does a value that is not a closure")
(assert (= 0 (length (fn/captures (fn [] 1)))) "a closure over nothing has none")

# ── The loader refuses mutable state in the value ──────────────────────

(assert (refused? '@[1 2]) "a mutable array")
(assert (refused? '@{:a 1}) "a mutable struct")
(assert (refused? '@"text") "a mutable string")
(assert (refused? '(@bytes 1 2)) "mutable bytes")
(assert (refused? '@|1 2|) "a mutable set")
(assert (refused? '(box 1)) "a box")
(assert (refused? '(fiber/new (fn [] 1) |:yield|)) "a fiber")
(assert (refused? '(make-parameter @[1])) "a parameter whose default is mutable")

(assert (refused? '[1 @[2]]) "a mutable array inside an array")
(assert (refused? '{:a {:b @[2]}}) "a mutable array inside a struct")
(assert (refused? '(list 1 @{})) "a mutable struct inside a list")

(assert (refused? '(let [@n 0]
                     (fn []
                       (assign n (+ n 1))
                       n))) "a closure over a binding it assigns")
(assert (refused? '(let [@n 0
                         bump (fn [] (assign n (+ n 1)))]
                     (fn [] (bump))))
        "a closure over a closure over a binding it assigns")
(assert (refused? '(let [items @[]]
                     (fn [x] (push items x))))
        "a closure over a binding whose value is mutable")
(assert (refused? '(let [items @[]]
                     {:add (fn [x] (push items x))}))
        "such a closure inside a struct")

# ── The loader allows what no code can change ─────────────────────────

(assert (= {:a [1 2] :b (list 3) :c |4|}
           (import/load-syntax '{:a [1 2] :b (list 3) :c |4|}))
        "immutable collections")
(assert (= 5
           ((import/load-syntax '(let [k 5]
                                   (fn [] k)))))
        "a closure over an immutable binding")
(assert (parameter? (import/load-syntax '(make-parameter [1])))
        "a parameter whose default is immutable")
(assert (closure? (import/load-syntax '(fn []
                                         (var n 0)
                                         (fn []
                                           (assign n 1)
                                           n))))
        "state built inside the function is the caller's own")
(let [m (import/load-syntax '(letrec [even (fn [n]
                                        (if (= n 0) true (odd (- n 1))))
                                      odd (fn [n]
                                        (if (= n 0) false (even (- n 1))))]
                               {:even even :odd odd}))]
  (assert (m:even 10) "closures that call each other through letrec"))

# The two loads of one counter module each build their own state, because
# the state lives inside the function the module answers.
(let [{:inc inc1 :count count1} ((import "../modules/counter"))
      {:inc inc2 :count count2} ((import "../modules/counter"))]
  (inc1)
  (inc1)
  (assert (= [2 0] [(count1) (count2)])
          "each call of the module's function owns its counter"))

# ── A module that answers a function is called first ───────────────────

# The counter-factual's message named the type alone, and left the reader to
# guess that the module had to be called.
(let [[ok? err] (protect (get (fn [] {:x 1}) :x))]
  (assert (not ok?) "get on a function fails")
  (assert (string/contains? (get err :message) "called before")
          "and says the module is called before its fields are read"))
