(elle/epoch 13)
# audited: 2026-09-29
# A fiber's I/O request passes through a mask that does not name :io, and a
# mask that names :io catches it.
#
# docs/signals/fibers.md
#
# A mask catches a signal by bit overlap. An I/O request carries :io alone:
# the primitive's static signature adds :error, but an error is an
# alternative return, not a bit the request carries. So neither |:error| nor
# |:error :yield| shares a bit with the request, and it passes through to
# the scheduler, which re-delivers the completion into the child.
#
# The counter-factual is a mask that catches :io by accident. The child would
# then pause on its own request and never complete, which S1 and S2 would see
# as a :paused status.

# S1: an |:error|-masked child's io passes through; the child completes.
(let [f (fiber/new (fn []
                     (println "io-mask: io from |:error| child")
                     :done) |:error|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "io passes through an |:error| mask")
  (assert (= (fiber/value f) :done) "the child completes past its io"))

# S2: overlap without :io does not catch — |:error :yield| still passes.
(let [f (fiber/new (fn []
                     (println "io-mask: io from |:error :yield| child")
                     :ok) |:error :yield|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) ":yield overlap alone does not trap io")
  (assert (= (fiber/value f) :ok) "the child completes past its io"))

# S3: a mask that names :io traps the request as a value.
(let [f (fiber/new (fn []
                     (println "io-mask: never printed")
                     :done) |:io|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "a mask naming :io traps the request"))

(println "io-mask: ok")
