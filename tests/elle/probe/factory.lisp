(elle/epoch 12)
# audited: 2026-09-08
# Call chains, a returned cycle, and the closure-as-module factories the direct-loop rows call, with the bindings they share.
#
# docs/impl/region/diagnostics.md
(defn helper-f [x]
  (string "v" x))
(defn helper-g [x]
  {:val x})
(defn helper-h [x]
  (+ x 1))
# `op` (a heap arg) is consumed only on the cold error path; on the success path
# its release would land in a branch the path never takes. Pins the cross-function
# per-path branch-compensation case (the check-comparable shape).
(defn check-arg [op a]
  (when (%not (number? a)) (string op " bad"))
  a)
(defn process [i]
  # called only through probe closures, so i is otherwise untyped
  (when (%not (%int? i)) (error :i-not-int))
  (make-struct (%add i 10)))
(defn t17-h []
  {:a 1})
(defn t17-h2 []
  {:b 2})
(defn cyc-mk []
  "A returned a<->b cycle — the transferred-returned-subtree shape (the
   %array-push stores keep the containment visible at this site in both
   intrinsics modes)."
  (let [a @[]
        b @[]]
    (%array-push a b)
    (%array-push b a)
    a))
(defn make-module []
  (defn mod-make [i]
    {:x i})
  (defn mod-label [i]
    (string "item-" i))
  {:make mod-make :label mod-label})
(defn make-heap-module []
  (defn do-process [i]
    {:x i :label (string "item-" i)})
  {:process do-process})
(def @t13proc (fn [i] {:x i}))
(def @t13cond (if (= :fast :fast) (fn [x] {:fast x}) (fn [x] {:slow x})))
(def @t13nested (fn [x] {:x x}))
(def the-mod (make-module))
(def heap-mod (make-heap-module))
(def @t19s @{:x 0})
(def @t20c 0)
# A signal the compiler cannot read as a keyword set, and a payload no fiber body
# allocates — the two ingredients a dynamic emit's borrowed park needs.
(def emit-sig :yield)
(def emit-error-sig :error)
(def emit-subject (string "emit-subject"))
