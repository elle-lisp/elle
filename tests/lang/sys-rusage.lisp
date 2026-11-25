(elle/epoch 18)
# audited: 2026-11-22
# sys/rusage answers what this whole process has cost, every thread included.
# docs/analysis/debugging.md
#
# The counter-factual for the thread test: a reading of the calling thread
# alone, which is what clock/cpu answers, sees nothing of a thread it joined.

(defn cost? [u]
  "Whether U has the three keys a cost answers with, each a count."
  (and (struct? u) (integer? (get u :user-us)) (integer? (get u :sys-us))
       (integer? (get u :max-rss-kb))))

(defn cpu-us [u]
  "The CPU time U holds, in user mode and in the kernel, in microseconds."
  (+ (get u :user-us) (get u :sys-us)))

(defn spin [seconds]
  "Burn this thread's CPU until its own clock has advanced SECONDS."
  (let [t0 (clock/cpu)]
    (while (< (- (clock/cpu) t0) seconds) nil)))

# A running process answers three counts, and holds memory.
(let [u (sys/rusage)]
  (assert (cost? u) "sys/rusage: answers three counts")
  (assert (> (get u :max-rss-kb) 0) "sys/rusage: a running process holds memory"))

# Work on this thread shows, and the peak never falls. The bound is half the
# time spent, so a slow clock tick cannot fail it.
(let [before (sys/rusage)]
  (spin 0.05)
  (let [after (sys/rusage)]
    (assert (>= (- (cpu-us after) (cpu-us before)) 25000)
            "sys/rusage: this thread's CPU time shows")
    (assert (>= (get after :max-rss-kb) (get before :max-rss-kb))
            "sys/rusage: the peak never falls")))

# Work on another thread shows too. What this thread spent while it waited is
# taken away, so only the other thread's time can pass the bound.
(let [before (sys/rusage)
      t0 (clock/cpu)]
  (os/join (os/spawn (fn []
                       (let [s (clock/cpu)]
                         (while (< (- (clock/cpu) s) 0.1) nil))
                       :done)))
  (let [after (sys/rusage)
        mine (integer (* 1000000.0 (- (clock/cpu) t0)))]
    (assert (>= (- (- (cpu-us after) (cpu-us before)) mine) 50000)
            "sys/rusage: another thread's CPU time shows")))

# It takes no argument.
(let [[ok? _] (protect ((fn [] (eval (read "(sys/rusage 1)")))))]
  (assert (not ok?) "sys/rusage: an argument is refused"))
