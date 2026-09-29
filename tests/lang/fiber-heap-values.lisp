(elle/epoch 13)
# audited: 2026-09-29
# A heap value a fiber yields, returns or raises stays readable to its resumer, across resumes and after the fiber ends.
# docs/signals/fibers.md
#
# Every fiber shares one heap, and a value outlives the fiber activation that
# built it for as long as a reader holds it. Each case hands a value out of a
# fiber and reads it back after the fiber has moved on: resumed again,
# finished, or raised. The counter-factual is a value that lives only as long
# as the fiber that built it, which reads back wrong, or not at all, once that
# fiber has moved on.

# ── One child ───────────────────────────────────────────────────────

(let* [f (fiber/new (fn ()
                      (yield (pair 1 2))
                      (pair 3 4)) 2)
       first-val (fiber/resume f)
       second-val (fiber/resume f)]
  (assert (= (first first-val) 1) "a yielded pair survives the next resume")
  (assert (= (first second-val) 3) "a returned pair survives the child"))

(let* [f (fiber/new (fn () (yield "hello")) 2)
       result (fiber/resume f)]
  (assert (= result "hello") "a yielded string reaches the resumer"))

(let* [f (fiber/new (fn ()
                      (yield "first")
                      (yield "second")
                      "done") 2)
       v1 (fiber/resume f)
       v2 (fiber/resume f)
       v3 (fiber/resume f)]
  (assert (= v1 "first") "first yield value")
  (assert (= v2 "second") "second yield value")
  (assert (= v3 "done") "final return value"))

# The yielded string outlives the child, which completes before the read.
(let* [f (fiber/new (fn ()
                      (yield "alive")
                      "done") 2)
       yielded (fiber/resume f)
       _ (fiber/resume f)]
  (assert (= yielded "alive") "a yielded value outlives the dead child"))

(let* [f (fiber/new (fn ()
                      (yield 42)
                      (yield (list 1 2 3))
                      (yield "end")) 2)]
  (let [v1 (fiber/resume f)
        v2 (fiber/resume f)
        v3 (fiber/resume f)]
    (assert (= v1 42) "mixed yields: an immediate")
    (assert (= (length v2) 3) "mixed yields: a list")
    (assert (= v3 "end") "mixed yields: a string")))

(let* [f (fiber/new (fn () (yield (list 10 20 30))) 2)
       lst (fiber/resume f)]
  (assert (= (first lst) 10) "yield list: first")
  (assert (= (first (rest lst)) 20) "yield list: second")
  (assert (= (first (rest (rest lst))) 30) "yield list: third"))

(let* [f (fiber/new (fn () (error "test error")) 1)
       _ (fiber/resume f)
       val (fiber/value f)]
  (assert (not (nil? val)) "a caught error's payload is readable"))

# ── Several fibers ──────────────────────────────────────────────────

(let* [c (fiber/new (fn () (yield "from-c")) 2)
       b (fiber/new (fn ()
                      (let* [val (fiber/resume c)]
                        (yield val))) 2)
       a-result (fiber/resume b)]
  (assert (= a-result "from-c") "a value yielded through a middle fiber"))

(let* [f1 (fiber/new (fn () (yield "from-f1")) 2)
       f2 (fiber/new (fn () (yield "from-f2")) 2)
       v1 (fiber/resume f1)
       v2 (fiber/resume f2)]
  (assert (= v1 "from-f1") "two children: the first value")
  (assert (= v2 "from-f2") "two children: the second value"))

(def sub
  (fiber/new (fn ()
               (yield "a")
               (yield "b")
               :done) |:yield|))
(def main (fiber/new (fn () (yield* sub)) |:yield|))
(fiber/resume main nil)
(def v1 (fiber/value main))
(fiber/resume main nil)
(def v2 (fiber/value main))
(assert (= v1 "a") "yield*: first")
(assert (= v2 "b") "yield*: second")

# ── Many resumes ────────────────────────────────────────────────────
# Fifty heap values, each yielded at its own resume, all read at the end.

(defn fiber-done? [f]
  (let [s (fiber/status f)]
    (or (= s :dead) (= s :error))))
(def @gen
  (fiber/new (fn ()
               (var i 0)
               (while (< i 50)
                 (yield (list i (+ i 1)))
                 (assign i (+ i 1)))) |:yield|))
(def @results @[])
(while (not (fiber-done? gen))
  (fiber/resume gen nil)
  (when (not (fiber-done? gen)) (push results (fiber/value gen))))
(assert (= (length results) 50) "long lived fiber: 50 yields")
(assert (= (first (get results 0)) 0) "long lived fiber: first yield")
(assert (= (first (get results 49)) 49) "long lived fiber: last yield")
