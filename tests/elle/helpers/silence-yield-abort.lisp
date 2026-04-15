# Helper script: called by silence-error.lisp to test runtime yield abort.
# This script should abort with "silence violation".
(defn bad [] (silence) (yield 1))
(bad)
