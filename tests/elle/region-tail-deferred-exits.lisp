(elle/epoch 12)
# A deferred tail-call release has the activation owner node's life
# (docs/impl/region/owner.md § "A deferred tail-call release has the node's
# life").
#
# A frame-replacing tail call strands the release the lowerer emitted past the
# `TailCall` — the callee closure's own per-call region — and the new activation
# takes it over. That obligation has to reach the activation's end whichever end
# it takes: the normal completion, a park it is resumed out of, or an
# abandonment no resume reaches.
#
# Every subject below is a per-call local closure called in TAIL position out of
# a body that then leaves by a signal. Its control is the same closure called in
# STATEMENT position, where no frame replacement happens and the compiler's own
# release runs — so the DIFFERENCE is the deferred release and nothing else.
#
# This file is the LEAK gauge — an `arena/count` delta over a fixed window. The
# soundness complement is region-tail-deferred-exits-uaf.lisp.

(def window 300)

(defn measure [thunk warm window]
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

(defn run-to-end [f]
  (fiber/resume f nil)
  (while (= (fiber/status f) :suspended) (fiber/resume f nil))
  nil)

# subjects ─────────────────────────────────────────────────────────────────────

# (a) an ERROR exit. The raising body IS the tail-called closure, so the
# activation that took the release over is the one the signal abandons.
(defn err-tail []
  (try
    (let [s (string "x" 1)]
      ((fn []
         (begin
           (error :boom)
           s))))
    (catch e nil)))

# (b) a PARK the fiber is resumed out of, all the way to completion. The
# ordinary case: no error anywhere, and the release still has to survive the
# suspend that unwound the loop holding it.
(defn park-resumed []
  (run-to-end (fiber/new (fn []
                           (let [s (string "x" 1)]
                             ((fn []
                                (begin
                                  (emit :yield 1)
                                  s))))) |:yield|)))

# (c) a PARK nothing resumes: the fiber handle is dropped while suspended, so
# the discharge is the release's last chance.
(defn park-dropped []
  (def f
    (fiber/new (fn []
                 (let [s (string "x" 1)]
                   ((fn []
                      (begin
                        (emit :yield 1)
                        s))))) |:yield|))
  (fiber/resume f nil)
  nil)

# (d) a SQUELCH boundary: the yield violates the mask, so the boundary raises a
# signal-violation the abandoned activation never catches.
(def squelched-tail
  (squelch (fn []
             (let [s (string "x" 1)]
               ((fn []
                  (begin
                    (emit :yield 1)
                    s))))) :yield))

(defn sq-tail []
  (try
    (squelched-tail)
    (catch e nil)))

# controls ─────────────────────────────────────────────────────────────────────

# The same closures called in statement position: no frame replacement, so the
# compiler's own release runs and nothing is deferred. A subject that reads like
# its control is measuring the deferral and not the exit's other accounting.
(defn err-nontail []
  (try
    (let [s (string "x" 1)]
      (begin
        ((fn []
           (begin
             (error :boom)
             s)))
        s))
    (catch e nil)))

(defn park-resumed-nontail []
  (run-to-end (fiber/new (fn []
                           (let [s (string "x" 1)]
                             (begin
                               ((fn []
                                  (begin
                                    (emit :yield 1)
                                    s)))
                               s))) |:yield|)))

(defn park-dropped-nontail []
  (def f
    (fiber/new (fn []
                 (let [s (string "x" 1)]
                   (begin
                     ((fn []
                        (begin
                          (emit :yield 1)
                          s)))
                     s))) |:yield|))
  (fiber/resume f nil)
  nil)

(def squelched-nontail
  (squelch (fn []
             (let [s (string "x" 1)]
               (begin
                 ((fn []
                    (begin
                      (emit :yield 1)
                      s)))
                 s))) :yield))

(defn sq-nontail []
  (try
    (squelched-nontail)
    (catch e nil)))

# (e) the clean break itself: the same tail call with no signal at all. Bounded
# before this mechanism and after it — a regression here is the deferred release
# failing to run where it always did.
(defn clean-tail []
  (let [s (string "x" 1)]
    ((fn [] s))))

# measurement ──────────────────────────────────────────────────────────────────

(def d-err (measure err-tail 20 window))
(def d-err-c (measure err-nontail 20 window))
(def d-park (measure park-resumed 20 window))
(def d-park-c (measure park-resumed-nontail 20 window))
(def d-drop (measure park-dropped 20 window))
(def d-drop-c (measure park-dropped-nontail 20 window))
(def d-sq (measure sq-tail 20 window))
(def d-sq-c (measure sq-nontail 20 window))
(def d-clean (measure clean-tail 20 window))

(println "region-tail-deferred-exits over " window " iters (object deltas):")
(println "  error exit     " d-err " (control " d-err-c ")")
(println "  park + resume  " d-park " (control " d-park-c ")")
(println "  park + drop    " d-drop " (control " d-drop-c ")")
(println "  squelch exit   " d-sq " (control " d-sq-c ")")
(println "  clean break    " d-clean " (control)")

# The trap: a control is not always 0. The squelch control reads one object per
# op, and it is NOT the boundary's own signal-violation error — a squelch of a
# body that allocates nothing reads 0. It is the body's own pending value: the
# squelch exit runs no abandoned-frame walk, so the release table's half of what
# that frame owed stays owed (docs/impl/region/mechanism.md § "An abandoned frame
# runs the releases it still owes" — a different mechanism, still open there).
# So each subject is read against its own control rather than against 0, with one
# window's slack for allocator noise.
(def slack 50)

(assert (%lt d-clean slack)
        (concat "control: the clean break must still run the deferred release, "
                "delta=" (number->string d-clean)))

(assert (%lt d-err (%add d-err-c slack))
        (concat "an abandoned activation owes its deferred release, delta="
                (number->string d-err) " control=" (number->string d-err-c)))
(assert (%lt d-park (%add d-park-c slack))
        (concat "a deferred release must survive a park the fiber is resumed "
                "out of, delta=" (number->string d-park) " control="
                (number->string d-park-c)))
(assert (%lt d-drop (%add d-drop-c slack))
        (concat "a park nothing resumes discharges its deferred release, delta="
                (number->string d-drop) " control=" (number->string d-drop-c)))
(assert (%lt d-sq (%add d-sq-c slack))
        (concat "a squelch boundary abandons the activation, so it owes the "
                "deferred release, delta=" (number->string d-sq) " control="
                (number->string d-sq-c)))

(println "region-tail-deferred-exits: ok")
