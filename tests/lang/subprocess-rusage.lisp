(elle/epoch 13)
# audited: 2026-09-30
# subprocess/rusage — a live sample of a running child, and the total a reap
# keeps on the subprocess.
# docs/subprocess.md

(defn cost? [u]
  "Whether U has the three keys a cost answers with, each a count."
  (and (struct? u) (integer? (get u :user-us)) (integer? (get u :sys-us))
       (integer? (get u :max-rss-kb))))

(defn spin [program]
  "A child running PROGRAM as a shell that loops forever on the CPU."
  (subprocess/exec program ["-c" "while :; do :; done"]))

(defn sample-once-busy [child]
  "Sample CHILD until its user time shows, giving up after about ten seconds.
   A busy child accrues user time within a clock tick of being scheduled, so
   the bound only covers a machine too loaded to schedule it; the last sample
   is the answer either way."
  (var u (subprocess/rusage child))
  (var tries 0)
  (while (and (< tries 200) (cost? u) (= (get u :user-us) 0))
    (ev/sleep 0.05)
    (assign u (subprocess/rusage child))
    (assign tries (+ tries 1)))
  u)

# A running child answers a sample of itself.
(let [napper (subprocess/exec "sleep" ["30"])
      u (subprocess/rusage napper)]
  (assert (cost? u) "subprocess/rusage: a running child answers a sample")
  (assert (> (get u :max-rss-kb) 0)
          "subprocess/rusage: a running child holds memory")
  (subprocess/kill napper :sigkill)
  (subprocess/wait napper))

# A child that burns CPU shows it while it runs, and the reaped total holds at
# least what the sample saw. The counter-factual for the sample is a reader
# that answers zeros; for the total, a reap that kept no usage.
(let [spinner (spin "sh")
      live (sample-once-busy spinner)]
  (assert (> (get live :user-us) 0)
          "subprocess/rusage: a busy child's sample counts its user time")
  (subprocess/kill spinner :sigkill)
  (subprocess/wait spinner)
  (let [total (subprocess/rusage spinner)]
    (assert (cost? total) "subprocess/rusage: a reaped child answers its total")
    (assert (>= (get total :user-us) (get live :user-us))
            "subprocess/rusage: the reaped total holds what the sample saw")))

# The reaped total covers the memory the child held, and it is kept: every
# later call answers the same struct.
(let [hog (subprocess/exec "sh"
                           ["-c"
                            "x=$(head -c 20000000 /dev/zero | tr '\\0' a); echo ${#x}"])]
  (subprocess/wait hog)
  (let [u (subprocess/rusage hog)]
    (assert (> (get u :max-rss-kb) 19000)
            "subprocess/rusage: the total covers the 20 MB the shell held")
    (assert (= u (subprocess/rusage hog))
            "subprocess/rusage: every later call answers the same total")))

# A child that has exited and that nothing has reaped may answer nil or a
# sample, and never raises.
(let [quick (subprocess/exec "true" [])]
  (ev/sleep 0.2)
  (let [[ok? u] (protect (subprocess/rusage quick))]
    (assert ok? "subprocess/rusage: an exited, unreaped child does not raise")
    (assert (or (nil? u) (cost? u))
            "subprocess/rusage: and answers nil or a sample"))
  (subprocess/wait quick))

# The trap: on Linux the sample reads /proc/PID/stat, whose second field is the
# command name in parentheses, and that name may itself hold spaces and
# parentheses. A reader that splits the line on spaces, or stops at the first
# `)`, takes its CPU times from fault counters that read zero for this child.
(with-temp-dir dir
               (let [odd (string dir "/a) R 9 (b")
                     sh (string/trim (get (subprocess/system "sh"
                                     ["-c" "command -v sh"]) :stdout))]
                 (subprocess/system "ln" ["-s" sh odd])
                 (let [spinner (spin odd)
                       u (sample-once-busy spinner)]
                   (assert (cost? u)
                           "subprocess/rusage: a child with an odd name answers")
                   (assert (> (get u :user-us) 0)
                           "subprocess/rusage: and its user time is read from its own field")
                   (subprocess/kill spinner :sigkill)
                   (subprocess/wait spinner))))

# A value that is not a subprocess is refused where it is passed.
(let [[ok? err] (protect (subprocess/rusage 42))]
  (assert (not ok?) "subprocess/rusage: an integer is refused")
  (assert (= (get err :error) :type-error)
          "subprocess/rusage: and refused as a type error"))
