(elle/epoch 15)
# audited: 2026-10-06
# A host that refuses a suspension raises at its own call, and a restart answers that call.
# docs/signals/primitives.md
#
# `eval` and `import` run code on the current fiber and cannot hold a
# suspension of it, so each answers one with an error. The fiber stops at the
# host's call like any raise, and the refused code is dropped.
#
# The counter-factual keeps the refused code's frames parked in the fiber. The
# error exit then parks no frame of the fiber's own, and a restart replays the
# refused code: its value ends the fiber, and the body past the host's call
# never runs.

# ── eval: a yield inside a call chain the evaluated code built ────────────────

(def by-eval
  (fiber/new (fn [] (list :got (eval '((fn [] (+ 1 (yield 1)))))))
             |:yield :error|))
(assert (= (get (fiber/resume by-eval) :error) :eval-error)
        "eval refuses a yield")
(assert (= (fiber/resume by-eval 41) (list :got 41))
        "a restart answers the eval call")
(assert (= (fiber/status by-eval) :dead) "and the body runs to its end")

# ── eval: an io park ─────────────────────────────────────────────────────────

(def by-eval-io
  (fiber/new (fn []
               (list :got (eval '(begin
                                   (ev/sleep 0)
                                   :inner)))) |:yield :error :io|))
(assert (= (get (fiber/resume by-eval-io) :error) :eval-error)
        "eval refuses an io park")
(assert (= (fiber/resume by-eval-io 41) (list :got 41))
        "a restart answers the eval call, not the refused sleep")

# ── import: a module whose top level yields ──────────────────────────────────

(with-temp-dir dir
               (let [mod (path/join dir "yields.lisp")]
                 (file/write mod "(elle/epoch 13)\n(yield 1)\n:module\n")
                 (let [f (fiber/new (fn [] (list :got (import mod)))
                                    |:yield :error :fs|)]
                   (assert (= (get (fiber/resume f) :error) :eval-error)
                           "import refuses a module that yields")
                   (assert (= (fiber/resume f 41) (list :got 41))
                           "a restart answers the import call"))))

# ── an error the hosted code raises ──────────────────────────────────────────
#
# An error parks every frame from the raise up, as a suspension does, and the
# host drops that chain the same way. The counter-factual leaves the hosted
# code's chain parked under the host's own error, and a restart replays the
# hosted code instead of answering the host's call.

(def eval-raise
  (fiber/new (fn [] (list :got (eval '((fn [] (+ 1 (error :inner))))))) |:error|))
(assert (= (get (fiber/resume eval-raise) :error) :eval-error)
        "eval reports the hosted error as its own")
(assert (= (fiber/resume eval-raise 41) (list :got 41))
        "a restart answers the eval call, not the raise inside it")

(with-temp-dir dir
               (let [mod (path/join dir "raises.lisp")]
                 (file/write mod
                             "(elle/epoch 15)\n(+ 1 (error {:error :inner}))\n:module\n")
                 (let [f (fiber/new (fn [] (list :got (import mod)))
                                    |:yield :error :fs|)]
                   (assert (= (get (fiber/resume f) :error) :eval-error)
                           "import reports a module's error as its own")
                   (assert (= (fiber/resume f 41) (list :got 41))
                           "a restart answers the import call"))))

(println "host-refusal: ok")
