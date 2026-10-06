(elle/epoch 15)
# audited: 2026-10-06
# A compiled callee's error parks its frame, so a restart answers the raising call inside it.
# docs/impl/jit.md
#
# The sidecar compiles each function on the VM thread at its first call, and
# the compiled code runs from the second call on. So each callee is warmed by
# one non-tail call before its case, and `jit?` confirms the compile; a tail
# call is not counted. tests/lang/fiber-restart.lisp holds the same cases,
# where the callee runs in the interpreter.
#
# The counter-factual is a compiled frame that unwinds on an error: the restart
# answers the interpreted body's call to the callee, and the frames between
# the raise and the body never run again.

(defn warm [thunk]
  (protect (begin
             (thunk)
             nil)))

(defn restarted [body value]
  (let [f (fiber/new body |:error|)]
    (fiber/resume f)
    (fiber/resume f value)))

# ── an emit, a primitive and an instruction raise in a compiled callee ──────

(defn raise-now []
  (+ 100 (error :boom)))
(defn get-now []
  (+ 100 (get nil :x)))
(defn first-now []
  (+ 100 (first nil)))
(defn match-now [v]
  (+ 100
     (match v
       1 1)))
(warm (fn [] [(raise-now)]))
(warm (fn [] [(get-now)]))
(warm (fn [] [(first-now)]))
(warm (fn [] [(match-now 5)]))
(assert (and (jit? raise-now) (jit? get-now) (jit? first-now) (jit? match-now))
        "the raising callees are compiled")

(assert (= (list :got 141) (restarted (fn [] (list :got (raise-now))) 41))
        "a restart answers the error inside a compiled callee")
(assert (= (list :got 141) (restarted (fn [] (list :got (get-now))) 41))
        "and a primitive's raise")
(assert (= (list :got 141) (restarted (fn [] (list :got (first-now))) 41))
        "and an instruction's raise")
(assert (= (list :got 141) (restarted (fn [] (list :got (match-now 5))) 41))
        "and a match no arm covers")

(let [f (fiber/new (fn [] (list :got (get-now))) |:yield|)]
  (assert (not (first (protect (fiber/resume f)))) "the error passes the resume")
  (assert (= (list :got 141) (fiber/resume f 41))
          "an :error fiber restarts inside the compiled callee"))

# ── compiled frames between the raise and the body ───────────────────────────

(defn outer-call []
  (+ 1000 (raise-now)))
(defn via-tail []
  (raise-now))
(warm (fn [] [(outer-call)]))
(warm (fn [] [(via-tail)]))
(assert (and (jit? outer-call) (jit? via-tail)) "the callers are compiled")

(assert (= (list :got 1141) (restarted (fn [] (list :got (outer-call))) 41))
        "every compiled frame between the raise and the body runs on")
(assert (= (list :got 141) (restarted (fn [] (list :got (via-tail))) 41))
        "a compiled tail call parks the callee it entered")

# ── a restart that raises again in compiled code ─────────────────────────────

(defn double [v]
  (* 2 (first v)))
(warm (fn [] [(double nil)]))
(assert (jit? double) "double is compiled")
(let [f (fiber/new (fn [] (list :got (double (get nil :x)))) |:error|)]
  (fiber/resume f)
  (assert (= :type-error (get (fiber/resume f nil) :error))
          "a nil restart raises in double")
  (assert (= (list :got 40) (fiber/resume f 20))
          "and the next restart answers the first inside double"))

# ── a destructure in a compiled callee ───────────────────────────────────────

(defn take-pair [x]
  (def [a b] x)
  (list :pair a b))
(warm (fn [] [(take-pair 7)]))
(assert (jit? take-pair) "take-pair is compiled")
(let [f (fiber/new (fn [] (list :got (take-pair 7))) |:error|)]
  (assert (= :type-error (get (fiber/resume f) :error))
          "the destructure raises in the callee")
  (assert (= :type-error (get (fiber/resume f [1 2]) :error))
          "the restart answers the first element, and the second raises")
  (assert (= (list :got (list :pair [1 2] 3)) (fiber/resume f 3))
          "each element answers its own restart"))

# ── parameterize in a compiled callee ────────────────────────────────────────

(def depth (make-parameter 0))
(defn bound-raise []
  (parameterize ((depth 9))
    (+ (depth) (get nil :x))))
(warm (fn [] [(bound-raise)]))
(assert (jit? bound-raise) "bound-raise is compiled")
(let [f (fiber/new (fn []
                     (list (parameterize ((depth 5))
                             (list (bound-raise) (depth))) (depth))) |:error|)]
  (fiber/resume f)
  (assert (= (list (list 50 5) 0) (fiber/resume f 41))
          "the compiled callee's binding stays in force across the restart"))

# A parameterize that raises has no result, so a restart continues past it.
(def not-a-param 42)
(defn bad-binding [flag]
  (when flag (error :never))
  (parameterize ((not-a-param 1))
    :x))
(warm (fn [] [(bad-binding false)]))
(assert (jit? bad-binding) "bad-binding is compiled")
(let [f (fiber/new (fn []
                     (list (parameterize ((depth 5))
                             (list (bad-binding false) (depth))) (depth)))
                   |:error|)]
  (assert (= :type-error (get (fiber/resume f) :error))
          "the callee's parameterize raises")
  (assert (= (list (list :x 5) 0) (fiber/resume f 41))
          "and the compiled callee runs on past it"))

# ── a bound check in a compiled callee ───────────────────────────────────────

# The yielding closure sits in an array, so the compiler cannot see its signal
# and leaves the bound to the check at run time, which has no result.
(defn apply-silent [g x]
  (bound g ||)
  (list :got (g x)))
(def hidden-yield (get @[(fn [x] (yield x))] 0))
(warm (fn [] [(apply-silent (fn [x] x) 1)]))
(assert (jit? apply-silent) "apply-silent is compiled")
(let [f (fiber/new (fn [] [(apply-silent hidden-yield 41)]) |:error :yield|)]
  (assert (= :signal-violation (get (fiber/resume f) :error)) "a bound raises")
  (assert (= 41 (fiber/resume f :ignored))
          "the restart continues past the check")
  (assert (= [(list :got 7)] (fiber/resume f 7)) "and the callee runs on"))

# ── a denied call in a compiled callee ───────────────────────────────────────

(defn measured [s]
  (def n (length s))
  (list :measured n))
(warm (fn [] [(measured "x")]))
(assert (jit? measured) "measured is compiled")
(let [f (fiber/new (fn []
                     (def r (measured "hello"))
                     (list :body r)) |:error| :deny |:error|)]
  (fiber/resume f)
  (assert (fiber/denied? f) "the length call is denied")
  (assert (= (list :body (list :measured 5)) (fiber/resume f 5))
          "the resume answers length, and both frames run on"))

(println "jit-error-park: ok")
