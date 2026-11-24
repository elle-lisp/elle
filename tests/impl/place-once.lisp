(elle/epoch 18)
# audited: 2026-11-23
# each places its body once and if-let its else form once, so a nest of each compiles in time linear in its depth.
# docs/macros/types.md
# docs/loops.md
#
# The counter-factual: an each that holds its body in one arm for each kind of
# collection places it four times. A nest then grows more than fourfold per
# level, and five levels compile more than ten times slower than three.

(def r ((import "std/ratchet")))

(defn count-in [form target]
  "How many times TARGET appears in the datum FORM."
  (cond
    (= form target) 1
    (pair? form)
      (+ (count-in (first form) target) (count-in (rest form) target))
    (or (array? form) (struct? form) (set? form))
      (sum (map (fn [x] (count-in x target)) (->array form)))
    0))

(defn nodes [form]
  "How many nodes the datum FORM holds."
  (cond
    (pair? form)
      (+ (nodes (first form)) (nodes (rest form)))
    (or (array? form) (struct? form) (set? form))
      (+ 1 (sum (map nodes (->array form))))
    1))

# ── Each argument once ───────────────────────────────────────────────

(assert (= (count-in (expand-macro '(each x in xs
                                      (body-mark x))) '(body-mark x)) 1)
        "each places its body once")

(def chain
  (expand-macro '(if-let [a 1
                          b 2
                          c 3]
                   (then-mark)
                   (else-mark))))
(assert (= (count-in chain '(else-mark)) 1) "if-let places its else form once")
(assert (= (count-in chain '(then-mark)) 1) "if-let places its then form once")

# ── A nest grows by the same amount at each level ────────────────────

(defn nest-form [depth]
  "DEPTH nested each loops over a around one println, as a datum."
  (let [@form '(println x)]
    (repeat depth
      (assign form (list 'each 'x 'in 'a form)))
    form))

(defn expanded [form]
  "FORM after expansion, as a datum. expand-macro expands only an argument the
  reader wrote with ', so the form goes through source text."
  (let [[[_ run]] (compile/whole-module (string "(expand-macro '" form ")")
                                        "<nest>")]
    (run)))

(let [sizes (map (fn [d] (nodes (expanded (nest-form d)))) [1 2 3 4])]
  (assert (= (- (get sizes 3) (get sizes 2)) (- (get sizes 2) (get sizes 1)))
          (string "each level adds the same nodes; sizes by depth were " sizes)))

# ── Compile time ─────────────────────────────────────────────────────
#
# Both callers pass a different kind of collection, so no type proof deletes
# an arm and the loop keeps every dispatch.

(defn nest-source [depth]
  (string "(defn visit [a] " (nest-form depth) ")\n"
          "(visit [1 2])\n(visit (list 1 2))"))

(defn compiling [depth]
  (let [src (nest-source depth)]
    (fn [] (compile/whole-module src "<place-once>"))))

(def shallow (compiling 3))
(def deep (compiling 5))
(shallow)
(def [control subject] (r:best-of 5 shallow deep))

(defn ms [seconds]
  (round (* seconds 1000.0)))

(println "place-once: depth 3 " (ms control) " ms, depth 5 " (ms subject) " ms")

(assert (< subject (* 4.0 (max control 0.0005)))
        (string "five nested loops must compile within four times three; they took "
                (ms subject) " ms against " (ms control) " ms"))
