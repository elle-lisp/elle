(elle/epoch 12)
# Soundness complement of region-tail-deferred-exits.lisp
# (docs/impl/region/owner.md § "A deferred tail-call release has the node's
# life"). Run under `--trace=guardfree` by the subprocess pin
# `region_tail_deferred_exits_uaf` in tests/integration/elle_scripts.rs.
#
# The deferred release now runs on paths that never ran it: an abandoned error
# exit, a squelch boundary, a park's discharge, and the completion of a body
# resumed out of a park. Each is a decref the activation genuinely owed, so what
# has to survive is every reference the activation does not own.
#
# Five faces.
#
# 1. The error PAYLOAD the raiser reached through the tail-called closure's
#    captured environment. The frame-exit relocation already ran the binding's
#    own release ahead of the `TailCall`, so the env's counted edge is the
#    value's last holder — and the deferred release cascades that edge away. The
#    payload survives on the reference the raise minted, and the catcher's read
#    proves it.
# 2. A closure that ESCAPES the frame that tail-called it. A container's counted
#    store holds it, so the deferred decref must not be the last reference.
# 3. A RESUMED body. The release belongs to the resumed body's completion, not
#    to the park, so everything the continuation still reads must be whole.
# 4. A DROPPED fiber's parked payload, released by the discharge. What
#    `fiber/value` hands back must survive that.
# 5. A tail-RECURSIVE loop, which re-enters with the same closure every
#    iteration and owes it ONE release. Releasing per iteration frees the
#    closure under the recursion still running in it.
#
# Every read below happens AFTER the exit ran, so an over-release faults at the
# deref (guardfree) or trips the generation check.

# ── 1. a payload reached through the tail callee's captured environment ───────

(defn raise-from-capture [tag]
  (let [[ok e] (protect (let [s (string "cap-" tag)]
                          ((fn [] (error s)))))]
    [ok (type-of e) (length e)]))

(var i 0)
(while (< i 40)
  (let [r (raise-from-capture i)]
    (assert (= (get r 0) false) "the tail-called closure must raise")
    (assert (= (get r 1) :string) "the captured payload must survive the exit")
    (assert (< 0 (get r 2)) "the captured payload's bytes must be whole"))
  (assign i (+ i 1)))

# ── 2. a tail-called closure a container still holds ──────────────────────────
# `sink` takes a counted reference before the call, so the deferred release
# drops the frame's own and no more. Reading the closure back after the raise —
# and CALLING it — faults if the release was the last.

(def sink @[])

(defn escape-then-raise [tag]
  (let [held (string "held-" tag)
        f (fn [] (error held))]
    (push sink f)
    (f)))

(assign i 0)
(while (< i 40)
  (protect (escape-then-raise i))
  (assign i (+ i 1)))

(assert (= (length sink) 40) "every escaping closure must have reached the sink")
(var n 0)
(while (< n (length sink))
  (let [[ok e] (protect ((get sink n)))]
    (assert (= ok false) "the stored closure must still be callable")
    (assert (= (type-of e) :string)
            "its capture must outlive the deferred release"))
  (assign n (+ n 1)))

# ── 3. a body resumed out of a park, running to completion ───────────────────
# The release belongs to the resumed body's completion. Running it at the park
# would free the closure the continuation is still executing in.

(defn park-then-finish [tag]
  (let [f (fiber/new (fn []
                       (let [s (string "park-" tag)]
                         ((fn []
                            (emit :yield 1)
                            (emit :yield 2)
                            (string s "-done"))))) |:yield|)]
    (fiber/resume f nil)
    (fiber/resume f nil)
    (fiber/resume f nil)
    (fiber/value f)))

(assign i 0)
(while (< i 40)
  (let [v (park-then-finish i)]
    (assert (= (type-of v) :string)
            "the resumed body's result must survive the deferred release")
    (assert (< 4 (length v)) "and must be whole, not a freed page"))
  (assign i (+ i 1)))

# ── 4. a dropped fiber's parked payload ──────────────────────────────────────
# Nothing resumes this fiber, so the discharge runs the deferred release. The
# payload it parked is read after that.

(defn park-then-drop [tag]
  (let [f (fiber/new (fn []
                       (let [s (string "drop-" tag)]
                         ((fn []
                            (begin
                              (emit :yield s)
                              s))))) |:yield|)]
    (fiber/resume f nil)
    (let [v (fiber/value f)]
      [(type-of v) (length v)])))

(assign i 0)
(while (< i 40)
  (let [r (park-then-drop i)]
    (assert (= (get r 0) :string)
            "the parked payload must survive the discharge")
    (assert (< 0 (get r 1)) "and must be whole"))
  (assign i (+ i 1)))

# ── 5. a squelch boundary, and the error it hands the catcher ────────────────

(def squelched
  (squelch (fn []
             (let [s (string "sq")]
               ((fn []
                  (begin
                    (emit :yield 1)
                    s))))) :yield))

(assign i 0)
(while (< i 40)
  (let [[ok e] (protect (squelched))]
    (assert (= ok false) "the squelch boundary must raise")
    (assert (= (type-of e) :struct)
            "the boundary's own error must survive the abandonment"))
  (assign i (+ i 1)))

# ── 6. one release per recursion, not one per iteration ──────────────────────
# A tail-recursive local closure re-enters with the SAME closure every step, and
# the activation owes it exactly one decref. A release per step frees the
# closure the recursion is still running in.

(defn count-down [n]
  (letrec [go (fn [k acc] (if (%lt k 1) acc (go (%sub k 1) (%add acc k))))]
    (go n 0)))

(assign i 0)
(while (< i 40)
  (assert (= (count-down 50) 1275) "the recursion must complete on one release")
  (assign i (+ i 1)))

# ...and the same recursion abandoned partway by an error, so the activation
# leaves through the walk while its deferred set names the one closure it
# re-entered 20 times.
(defn count-down-raising [n]
  (letrec [go (fn [k acc]
                (if (%lt k 30) (error acc) (go (%sub k 1) (%add acc k))))]
    (go n 0)))

(assign i 0)
(while (< i 40)
  (let [[ok e] (protect (count-down-raising 50))]
    (assert (= ok false) "the recursion must raise")
    (assert (= (type-of e) :integer) "and hand the catcher its accumulator"))
  (assign i (+ i 1)))

(println "region-tail-deferred-exits-uaf: ok")
