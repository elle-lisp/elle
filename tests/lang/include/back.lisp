(elle/epoch 15)
# audited: 2026-10-21
# The helper include.lisp splices in: functions that name what the including file defines.
# docs/modules.md

(defn ask-includer []
  "What includer-answer, which the including file defines, answers."
  (includer-answer))

(defn includer-base-plus [n]
  "n more than includer-base, which the including file defines."
  (+ includer-base n))
