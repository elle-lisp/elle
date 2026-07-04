(elle/epoch 12)
## A file that does not compile: an unbound reference. Whole-module analysis
## fails, so the barrier compile reports a single FILE-LEVEL failure (a file
## that won't compile has no forms to run) and the gate exits non-zero.
(assert (= (this-symbol-is-not-defined-anywhere 1) 1) "never runs")
