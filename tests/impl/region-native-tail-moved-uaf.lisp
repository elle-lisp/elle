(elle/epoch 18)
# audited: 2026-11-20
# A pass-through result returned from a moved native tail call outlives its container on the Return node's mint.
# docs/impl/region/returnmint.md
#
# Each subject returns `first`/`get` of a container the caller builds per call,
# from a position where a HIR `Return` node, not the native's fall-through
# retain, mints the caller's reference: a tail branch's arm beside an assign,
# and the last operand of a tail `and`/`or`. The container dies at the call, so
# the result's only owning reference is that mint. The counter-factual: a
# lowering that stands the fall-through retain down where no `Return` node
# follows leaves the result with none, and the caller's read faults under
# `--trace=guardfree`.
#
# The trap: guardfree leaks one PROT_NONE mapping per freed page, so a loop much
# past a few hundred calls aborts on the mapping budget instead of a fault. A
# subject called by name is grafted into its caller, where no tail call exists,
# so each is reached through a reassigned binding instead.

(var slot nil)
(defn opaque [f]
  "F, behind a binding the graft cannot resolve."
  (assign slot f)
  slot)

(def if-first
  (opaque (fn [xs]
            (var n 0)
            (if (nil? xs)
              7
              (begin
                (assign n 1)
                (first xs))))))
(def cond-get
  (opaque (fn [xs]
            (var n 0)
            (cond
              (nil? xs) 7
              true
                (begin
                  (assign n 1)
                  (get xs 0))))))
(def match-first
  (opaque (fn [xs]
            (var n 0)
            (match xs
              nil 7
              _
                (begin
                  (assign n 1)
                  (first xs))))))
(def or-first (opaque (fn [xs] (or nil (first xs)))))
(def and-get (opaque (fn [xs] (and true (get xs 0)))))

(var i 0)
(var a nil)
(var b nil)
(var c nil)
(var d nil)
(var e nil)
(while (< i 400)
  (assign a (if-first (list (concat "xy" "z"))))
  (assign b (cond-get [(concat "xy" "z")]))
  (assign c (match-first (list (concat "xy" "z"))))
  (assign d (or-first (list (concat "xy" "z"))))
  (assign e (and-get @[(concat "xy" "z")]))
  (assign i (+ i 1)))

(assert (= a "xyz") "an if arm beside an assign returns first of a dead list")
(assert (= b "xyz") "a cond arm beside an assign returns get of a dead array")
(assert (= c "xyz") "a match arm beside an assign returns first of a dead list")
(assert (= d "xyz") "the last operand of a tail or returns first of a dead list")
(assert (= e "xyz") "the last operand of a tail and returns get of a dead array")

(println "region-native-tail-moved-uaf: ok")
