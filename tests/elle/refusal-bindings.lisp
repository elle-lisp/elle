(elle/epoch 13)
# audited: 2026-09-29
# A refused park drops the `parameterize` bindings its code made, so the code around the refusal runs on with its own.
#
# docs/signals/primitives.md
#
# A squelch boundary and a host that runs code on the current fiber each
# refuse a suspension, and the refused frames never run again. The frames
# pushed parameter bindings and pop none of them. The counter-factual leaves
# those bindings on the fiber: a restart answers the refused call, and the
# code after it reads the refused code's binding (9) instead of its own.

(def depth (make-parameter 0))
(defn bound-yield []
  (parameterize ((depth 9))
    (yield 1)
    :inner))
(defn bound-raise []
  (parameterize ((depth 9))
    (+ 1 (error :boom))))

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
        "a squelch at an interpreted call drops the refused binding")

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

(assert (= (restart (fn []
                      (list :got (compile/run-on :bytecode squelched) (depth))))
           (list :got 41 0))
        "a squelch under compile/run-on :bytecode drops the refused binding")

(assert (= (restart (fn [] (list :got (compile/run-on :jit squelched) (depth))))
           (list :got 41 0))
        "a squelch under compile/run-on :jit drops the refused binding")

## ── a host that refuses ─────────────────────────────────────────────

(assert (= (restart (fn []
                      (list :got (eval '(bound) {'bound bound-yield}) (depth))))
           (list :got 41 0)) "eval drops the refused binding")

(assert (= (restart (fn [] (list :got (compile/run-on :jit bound-yield) (depth))))
           (list :got 41 0)) "compile/run-on :jit drops the refused binding")

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

## ── an error under compile/run-on :jit ──────────────────────────────

# The compiled closure's frames leave by the error and pop no binding, so the
# host truncates to the depth at its entry, as every other host does.
(assert (= (restart (fn [] (list :got (compile/run-on :jit bound-raise) (depth))))
           (list :got 41 0))
        "an error under compile/run-on :jit drops the raising code's binding")
