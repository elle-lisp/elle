(elle/epoch 15)
# audited: 2026-10-13
# A lambda inside a function that refers to itself carries the signal of that
# function, wherever the function is defined. Counter-factual: a defn inside a
# function body read its own silent seed while it was analyzed, so the lambda
# that calls it inferred silent, and its yield broke the proof at run time.

(defn compiles? [form]
  (get (protect (eval form)) 0))

# ── A defn inside a function body ────────────────────────────────────

(defn outer []
  (defn walk [n]
    (if (= n 0)
      (yield :leaf)
      (list :back ((fn [k] (walk k)) (- n 1)))))
  walk)

(def walker (fiber/new (fn [] ((outer) 3)) |:yield|))
(assert (= :leaf (fiber/resume walker)) "the first resume answers the yield")
(fiber/resume walker)
(assert (= :dead (fiber/status walker)) "the second resume finishes the walk")
(assert (= '(:back (:back (:back nil))) (fiber/value walker))
        "each lambda returns into the walk that called it")

# ── A defn inside a let body ─────────────────────────────────────────

(defn outer-let []
  (let [mark :leaf]
    (defn walk [n]
      (if (= n 0)
        (yield mark)
        (list :back ((fn [k] (walk k)) (- n 1)))))
    walk))

(def let-walker (fiber/new (fn [] ((outer-let) 2)) |:yield|))
(assert (= :leaf (fiber/resume let-walker))
        "a defn in a let body converges like one in a function body")

# ── A ceiling on the lambda sees the signal ──────────────────────────

(assert (not (compiles? '(defn outer []
                          (defn walk [n]
                            (if (= n 0)
                              (yield n)
                              ((fn [k]
                                 (silent!)
                                 (walk k)) (- n 1))))
                          walk)))
        "silent! rejects a lambda that calls a yielding walk in a function body")

(assert (not (compiles? '(begin
                           (defn walk [n]
                             (if (= n 0)
                               (yield n)
                               ((fn [k]
                                  (silent!)
                                  (walk k)) (- n 1))))
                           walk)))
        "silent! rejects the same lambda in a form compiled alone")

# Convergence must not inflate a silent self-reference into a signalling one.

(assert (compiles? '(defn outer []
                     (defn tally [n]
                       (if (%eq n 0)
                         0
                         ((fn [k]
                            (silent!)
                            (tally k)) (%sub n 1))))
                     tally))
        "silent! accepts a lambda that calls a silent tally")
