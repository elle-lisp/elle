(elle/epoch 14)
# audited: 2026-10-06
# A function compiles once its calls reach the threshold the run started with, and stays interpreted below it.
# docs/config.md
#
# The sidecar keeps every call a call, so the loops below reach their functions
# rather than their bodies, and compiles on the VM thread, so `jit?` does not
# race the `elle-jit` worker. The rig runs this file under the build's default
# threshold, and again under the eager profile, where every function compiles
# on its first call.
#
# The counter-factual: a threshold the dispatcher never consults still reads
# back from `vm/config`. So each function is driven past the threshold, or held
# under it, and asked whether native code exists for it. A `jit?` that answered
# true for every function would pass the hot case alone, so the cold case
# stands beside it.
#
# A build without the JIT reads nil and compiles nothing, so it asserts nothing.

(def threshold (vm/config :jit))

(defn hot [n]
  (+ n 1))
(defn cold [n]
  (+ n 1))
(defn warm [n]
  (+ n 1))

# Call `f` `calls` times.
(defn drive [f calls]
  (def @i 0)
  (while (< i calls)
    (f i)
    (assign i (+ i 1))))

(when threshold
  (drive hot 50)
  (assert (jit? hot) "50 calls compile a function under the starting threshold")
  (drive cold 3)
  (if (= threshold 0)
    (assert (jit? cold) "the eager profile compiles on the first call")
    (assert (not (jit? cold)) "3 calls stay under the build's threshold of 10"))
  # The threshold is the one JIT setting a program owns. Set to one, three calls
  # compile what the build's threshold leaves interpreted.
  (vm/config-set :jit 1)
  (drive warm 3)
  (assert (jit? warm) "a threshold of one compiles after three calls"))
