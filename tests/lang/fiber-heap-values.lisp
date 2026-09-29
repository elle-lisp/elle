(elle/epoch 13)
# audited: 2026-09-29
# A heap value a fiber yields, returns or raises stays readable to its resumer, across resumes and after the fiber ends.
# docs/signals/fibers.md
#
# Each case hands a value out of a fiber and reads it back after the fiber has
# moved on: resumed again, finished, raised, been cancelled or aborted. The
# counter-factual is a value that lives only as long as the fiber that built
# it, which reads back wrong, or not at all, once that fiber has moved on.

# Values allocated in a child survive across its yield/resume cycles.
(let* [f (fiber/new (fn ()
                      (yield (pair 1 2))
                      (pair 3 4)) 2)
       first-val (fiber/resume f)
       second-val (fiber/resume f)]
  (assert (= (first first-val) 1) "first yield value first element")
  (assert (= (first second-val) 3) "second yield value first element"))

(let* [f (fiber/new (fn () (yield "hello")) 2)
       result (fiber/resume f)]
  (assert (= result "hello") "yielding child yields string"))

# A child that never yields returns an immediate.
(let* [f (fiber/new (fn () 42) 1)
       result (fiber/resume f)]
  (assert (= result 42) "non-yielding child returns immediate"))

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

# A→B→C: C yields a string, B catches it and yields it on to A.
(let* [c (fiber/new (fn () (yield "from-c")) 2)
       b (fiber/new (fn ()
                      (let* [val (fiber/resume c)]
                        (yield val))) 2)
       a-result (fiber/resume b)]
  (assert (= a-result "from-c") "abc chain yield through"))

# Root → child → grandchild: the grandchild yields, the child yields it on.
(let* [gc (fiber/new (fn () (yield "from-gc")) 2)
       child (fiber/new (fn ()
                          (let* [val (fiber/resume gc)]
                            (yield val))) 2)]
  (let [result (fiber/resume child)]
    (assert (= result "from-gc") "root child grandchild yield")))

# A child yields a string and then finishes; the yielded string outlives it.
(let* [f (fiber/new (fn ()
                      (yield "alive")
                      "done") 2)
       yielded (fiber/resume f)
       _ (fiber/resume f)]
  (assert (= yielded "alive") "child death value survives"))

(let* [f (fiber/new (fn ()
                      (yield 0)
                      (yield 1)
                      (yield 2)) 2)]
  (let [v1 (fiber/resume f)
        v2 (fiber/resume f)
        v3 (fiber/resume f)]
    (assert (= v1 0) "multi resume yield basic: first")
    (assert (= v2 1) "multi resume yield basic: second")
    (assert (= v3 2) "multi resume yield basic: third")))

(let* [f (fiber/new (fn ()
                      (yield "hello")
                      (yield "world")
                      (yield "done")) 2)]
  (let [v1 (fiber/resume f)
        v2 (fiber/resume f)
        v3 (fiber/resume f)]
    (assert (= v1 "hello") "multi resume heap: first")
    (assert (= v2 "world") "multi resume heap: second")
    (assert (= v3 "done") "multi resume heap: third")))

(let* [f (fiber/new (fn ()
                      (yield 42)
                      (yield (list 1 2 3))
                      (yield "end")) 2)]
  (let [v1 (fiber/resume f)
        v2 (fiber/resume f)
        v3 (fiber/resume f)]
    (assert (= v1 42) "multi resume mixed: first")
    (assert (= (length v2) 3) "multi resume mixed: second is list")
    (assert (= v3 "end") "multi resume mixed: third")))

# Two different children each yield a string, and both stay readable.
(let* [f1 (fiber/new (fn () (yield "from-f1")) 2)
       f2 (fiber/new (fn () (yield "from-f2")) 2)
       v1 (fiber/resume f1)
       v2 (fiber/resume f2)]
  (assert (= v1 "from-f1") "multiple children: first")
  (assert (= v2 "from-f2") "multiple children: second"))

(let* [f (fiber/new (fn () (yield 42)) 2)
       result (fiber/resume f)]
  (assert (= result 42) "yield immediate"))

# The parent walks every cell of a yielded list.
(let* [f (fiber/new (fn () (yield (list 10 20 30))) 2)
       lst (fiber/resume f)]
  (assert (= (first lst) 10) "yield list: first")
  (assert (= (first (rest lst)) 20) "yield list: second")
  (assert (= (first (rest (rest lst))) 30) "yield list: third"))

# yield* delegates to a sub-fiber, and each value it passes on stays readable.
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
(assert (= v1 "a") "yield star: first")
(assert (= v2 "b") "yield star: second")

# A child raises an error; the parent reads the error value afterwards.
(let* [f (fiber/new (fn () (error "test error")) 1)
       _ (fiber/resume f)
       val (fiber/value f)]
  (assert (not (nil? val)) "error in child is readable"))

# The parent cancels a suspended child. Mask 3 catches both :error (1) and
# :yield (2), so the cancel does not propagate.
(let* [f (fiber/new (fn ()
                      (yield "yielded")
                      "never-reached") 3)
       v1 (fiber/resume f)]
  (fiber/cancel f "cancelled")
  (let [status (string (fiber/status f))]
    (assert (= v1 "yielded") "cancel child: yielded value")
    (assert (= status "error") "cancel child: status is error")))

# Aborting a fiber that never ran, with a heap value: the fiber ends :error and
# the abort answers the value.
(let* [f (fiber/new (fn () "never-reached") 1)
       v (fiber/abort f [:k :reason])]
  (assert (= (string (fiber/status f)) "error")
          "abort new fiber: status is error")
  (assert (= v [:k :reason]) "abort new fiber: heap error value passed through"))

# A fiber resumed 50 times, yielding a fresh list each time: every value is
# readable at the end.
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
