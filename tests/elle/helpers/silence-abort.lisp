# Helper script: called by silence-error.lisp to test runtime abort.
# This script should abort with "silence violation".
(defn bad-add [x y]
  (silence)
  (+ x y))

(bad-add "not" "numbers")
