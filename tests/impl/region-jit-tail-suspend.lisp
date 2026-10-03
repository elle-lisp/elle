(elle/epoch 13)
# audited: 2026-09-29
# A compiled function whose tail call suspends parks its continuation at the tail call, and the resume runs the block after it.
# docs/impl/region/park.md
#
# The trap: the function's value is right either way. A compiled frame that
# returns at the tail call instead of parking hands the resume value to its
# caller's call site, which is exactly where it belongs. What goes missing is
# the block after the tail call: the release of each owned parameter, so every
# heap argument of every suspended call stays live. Only a gauge sees that.
#
# The counter-factual for the gauge's other side is a block that runs twice, or
# a park that releases a value its caller still holds. Each face therefore
# reads its argument after the resume, and
# region-jit-tail-suspend-guardfree.lisp runs this file with guardfree armed,
# which faults on a freed one.
#
# The interpreter parks the continuation itself. The gauge waits for the JIT's
# compiles before it measures, so the functions below run compiled in the
# window. region-jit-tail-suspend-syncjit.lisp runs this file with each
# function compiled on its first call, on the VM thread.

# ── the compiled tail calls ─────────────────────────────────────────────────
# Each makes a suspending native call in tail position, with a heap argument
# the function owns as a parameter.

# A dynamic emit: a keyword in a parameter lowers to the `emit` primitive.
(defn raise [kw v]
  (emit kw v))

# An io op, which parks its request.
(defn put [p s]
  (port/write p s))

# A fiber resume whose child's yield its mask does not catch.
(defn step [f v]
  (fiber/resume f v))

# ── the faces ───────────────────────────────────────────────────────────────
(defn yield-face [n]
  (let [f (fiber/new (fn []
                       (let [x (string "held " n)
                             r (raise :yield x)]
                         (assert (= x (string "held " n))
                                 "a raised argument was freed under its caller")
                         r)) |:yield|)]
    (fiber/resume f)
    (assert (= (fiber/resume f :answer) :answer)
            "the resume value is the tail call's result")))

(defn io-face [p n]
  (let [s (string "line " n "\n")]
    (put p s)
    (assert (= s (string "line " n "\n"))
            "a written argument was freed under its caller")))

(defn carrier-face [n]
  (let [outer (fiber/new (fn []
                           (let [g (fiber/new (fn []
                                   (yield (string "from " n))
                                   :child-done) |:error|)
                                 r (step g (string "ignored " n))]
                             (assert (= r :child-done)
                                     "the child's result is the tail resume's result")
                             r)) |:yield|)]
    (assert (= (fiber/resume outer) (string "from " n))
            "the child's yield passes through the tail resume")
    (assert (= (fiber/resume outer :go) :child-done)
            "the replayed resume finishes the child")))

(defn denial-face [p n]
  (let [f (fiber/new (fn []
                       (let [s (string "denied " n)
                             r (put p s)]
                         (assert (= s (string "denied " n))
                                 "a denied call's argument was freed under its caller")
                         r)) |:io :error| :deny |:io|)]
    (fiber/resume f)
    (assert (= (fiber/resume f 7) 7)
            "the mediator's answer is the denied tail call's result")))

# The control: the same raise in call position, which a compiled frame parks
# at its call site.
(defn raise-call [kw v]
  (let [r (emit kw v)]
    r))

(defn control-face [n]
  (let [f (fiber/new (fn []
                       (let [x (string "held " n)]
                         (raise-call :yield x))) |:yield|)]
    (fiber/resume f)
    (fiber/resume f :answer)))

# ── drive ───────────────────────────────────────────────────────────────────
(def window 200)

(defn measure [thunk]
  (def @i 0)
  (while (%lt i 30)
    (thunk i)
    (assign i (%add i 1)))
  # Compiles run on a worker; wait for them, so the window runs compiled code.
  (jit/rejections)
  (def before (arena/count))
  (def @j 0)
  (while (%lt j window)
    (thunk j)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

# A lost block strands at least one object per suspended call, so a leak reads
# at least `window`.
(defn bounded? [d label]
  (assert (%lt d 60) (string label " leaks, delta=" d)))

(with-temp-dir dir
               (let [p (port/open (path/join dir "out.txt") :write)]
                 (bounded? (measure control-face)
                           "control: a raise in call position")
                 (bounded? (measure yield-face) "a raise in tail position")
                 (bounded? (measure (fn [n] (io-face p n)))
                           "a write in tail position")
                 (bounded? (measure carrier-face)
                           "a fiber resume in tail position")
                 (bounded? (measure (fn [n] (denial-face p n)))
                           "a denied write in tail position")
                 (port/close p)))

(println "region-jit-tail-suspend: ok")
