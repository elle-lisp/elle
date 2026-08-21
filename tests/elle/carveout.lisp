(elle/epoch 12)
# The :io carve-out in the mask catch test.
#
# A mask catches a compound signal by bit overlap, with one exception
# (`covers`, src/value/fiber/signalbits.rs): a compound that carries :io
# is caught only by a mask that names :io. An io request is emitted as
# |:io :yield|, so a child that masks |:error| — or even |:error :yield| —
# does not trap its own io. The request passes through to the root's io
# machinery, and the FiberResume chain (src/vm/fiber/resume.rs) re-delivers
# the completion into the child.
#
# The debugger leans on this shape: a debuggee masked |:debug :fuel :error|
# keeps doing io while it runs under a driver (docs/debugger.md
# § "Architecture").

# S1: an |:error|-masked child's io passes through; the child completes.
(let [f (fiber/new (fn []
                     (println "carveout: io from |:error| child")
                     :done) |:error|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "io passes through an |:error| mask")
  (assert (= (fiber/value f) :done) "the child completes past its io"))

# S2: overlap without :io does not catch — |:error :yield| still passes.
(let [f (fiber/new (fn []
                     (println "carveout: io from |:error :yield| child")
                     :ok) |:error :yield|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) ":yield overlap alone does not trap io")
  (assert (= (fiber/value f) :ok) "the child completes past its io"))

# S3: a mask that names :io traps the request as a value.
(let [f (fiber/new (fn []
                     (println "carveout: never printed")
                     :done) |:io|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "a mask naming :io traps the request"))

(println "carveout: ok")
