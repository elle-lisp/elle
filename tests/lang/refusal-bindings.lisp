(elle/epoch 13)
# audited: 2026-09-29
# A refused park drops the `parameterize` bindings its code made, so the code around the refusal runs on with its own.
# docs/signals/primitives.md
#
# A squelch boundary and a host that runs code on the current fiber each
# refuse a suspension, and the refused frames never run again. The frames
# pushed parameter bindings and pop none of them. The counter-factual leaves
# those bindings on the fiber: a restart answers the refused call, and the
# code after it reads the refused code's binding (9) instead of its own.
#
# tests/impl/refusal-bindings-run-on.lisp holds the same refusals under
# `compile/run-on`, an extension of this implementation.

(def depth (make-parameter 0))
(defn bound-yield []
  (parameterize ((depth 9))
    (yield 1)
    :inner))

(defn restart [body]
  "Run `body` in a fiber that stops on the refusal, restart it with 41, and
  answer what the body ends with."
  (let [f (fiber/new body |:yield :error|)]
    (fiber/resume f)
    (assert (= (fiber/bits f) 1) "the refusal stops the fiber on an error")
    (fiber/resume f 41)))

## ── a squelch boundary ──────────────────────────────────────────────

(def squelched (squelch bound-yield :yield))

(assert (= (restart (fn [] (list :got (squelched) (depth)))) (list :got 41 0))
        "a squelch at a call drops the refused binding")

(assert (= (restart (fn []
                      (list :got (parameterize ((depth 5))
                                   (squelched)) (depth)))) (list :got 41 0))
        "a squelch inside a parameterize leaves that parameterize's own frame")

(assert (= (restart (fn []
                      (list :got (parameterize ((depth 5))
                                   (list (squelched) (depth))))))
           (list :got (list 41 5)))
        "the enclosing parameterize still binds after the refusal")

# The boundary of a tail call ends the calling activation too, and that
# activation's caller drops the binding when the error reaches it.
(assert (= (restart (fn [] (list :got ((fn [] (squelched))) (depth))))
           (list :got 41 0))
        "a squelch at a tail call drops the refused binding")

## ── a host that refuses ─────────────────────────────────────────────

(assert (= (restart (fn []
                      (list :got (eval '(bound) {'bound bound-yield}) (depth))))
           (list :got 41 0)) "eval drops the refused binding")

# A module sees no binding of this file, so it binds a built-in parameter.
(with-temp-dir dir
               (let [mod (path/join dir "binds.lisp")]
                 (file/write mod
                             "(elle/epoch 13)\n(parameterize ((*io-keepalive* 7)) (yield 1))\n:module\n")
                 (let [f (fiber/new (fn []
                                      (list :got (import mod) (*io-keepalive*)))
                                    |:yield :error :fs|)]
                   (fiber/resume f)
                   (assert (= (fiber/resume f 41) (list :got 41 nil))
                           "import drops the refused binding"))))
