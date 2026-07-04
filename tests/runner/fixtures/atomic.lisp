(elle/epoch 12)
## Atomicity of the whole-file (single-thunk) legacy mode.
##
## A multi-form legacy file is ONE form: the FIRST failing assert aborts the
## rest, exactly as a direct `elle FILE` run does. The per-form non-abort
## isolation that the old barrier mode gave each top-level form is deliberately
## gone for multi-form files — it was precisely what reordered ordered scripts
## (see multi.lisp). The whole file records a single `:failed-assertion` result,
## labelled by the first assert message, and the second assert never runs.
(def x 1)
(assert (= x 2) "first failure aborts the file")
(assert (= x 1) "unreachable — the file aborted at the first failure")
