(elle/epoch 18)
## audited: 2026-11-22
## What a form killed by its deadline leaves behind: its output, and a photograph of every thread.
## tests/runner/overview.md
## docs/test-runner.md
## docs/testing.md
##
## `timeout … join: deadline exceeded` names the budget, not the call the form
## was in when the budget ran out. The form's own output is one account of
## that, and it is already written when the form wedges. `size` on the asset
## row is the uncompressed length, so the exact line is pinned without reading
## the CAS entry.
##
## Each fixture prints and waits far past the deadline in a single form. The
## runner's unit is the form, so a print in a form of its own would belong to a
## result that never timed out.

(include-file "lib/harness.lisp")

(def marker "before-the-hang")

(defn sampler? []
  "Whether this box has the sampler the runner photographs with: `sample`
   ships with macOS, and `eu-stack` comes with elfutils."
  (or (= (string/trim ((subprocess/system "uname" ["-s"]) :stdout)) "Darwin")
      (let [[ok? r] (protect (subprocess/system "eu-stack" ["--version"]))]
        (and ok? (= r:exit 0)))))

(defn thread-headers [photo]
  "The thread headers in a photograph: `TID 123:` from eu-stack, and
   `611 Thread_123 …` from sample, whose first word is a sample count."
  (length (filter (fn [l]
                    (let [words (filter (fn [w] (not (empty? w)))
                                        (string/split l " "))]
                      (or (string/starts-with? l "TID ")
                          (and (> (length words) 1)
                               (string/starts-with? (get words 1) "Thread_")))))
                  (string/split photo "\n"))))

(with-temp-dir dir
  (defn run-wedged [name form]
    "Run one fixture holding FORM, which must wedge past a two-second budget,
     and answer the run with its stdout and stderr joined as :said."
    (let [r (elle-test (path/join dir name "s.db")
                       ["--timeout" "2000"
                        (write-fixture dir (string name ".lisp") form)])]
      (assert (not (= r:exit 0)) (string "a wedged form gates non-zero: " r:out))
      (put r :said (string r:out r:err))))

  (file/mkdir-all (path/join dir "hang"))
  (def hang
    (run-wedged "hang"
                (string "(begin (eprintln \"" marker "\") (ev/sleep 600))\n")))

  (eprintln "scenario: a timed-out form keeps the output it produced")
  (assert (string/contains? hang:said "timeout")
          (string "the fixture must be recorded as a timeout: " hang:said))
  (let [res (rows hang:db
                  (string "SELECT a.kind AS kind, a.size AS size "
                          "FROM asset a JOIN result r "
                          "ON r.id = a.result_id WHERE "
                          "r.status = 'timeout' AND a.kind = 'stderr'"))]
    (assert (not (empty? res)) "a timed-out form must carry its stderr")
    (assert (all? (fn [row] (= row:size (+ (length marker) 1))) res)
            (string "the asset must hold exactly what the form printed: " res)))

  # The problem list is what a CI log shows, and the last line the form
  # printed names the call it was in when the budget ran out.
  (eprintln "scenario: the problem list names the last line a timed-out form printed")
  (assert (string/contains? hang:said marker)
          (string "the timeout's problem line must carry the form's last "
                  "output: " hang:said))

  # The runner redirects each form and tier to scratch/*.out and *.err beside
  # the DB, and deletes the pair once it has the bytes. An abandoned worker
  # still holds its end open, which POSIX allows: the unlink is what matters.
  (eprintln "scenario: a timed-out form leaves no capture files behind")
  (let [scratch (path/join (path/parent hang:db) "scratch")
        left (if (file/directory? scratch) (file/ls scratch) [])]
    (assert (empty? left)
            (string "the capture files must not outlive the run: " left)))

  # The photograph is for the one thread that wedged, and the runner cannot
  # know which thread that is, so it prints every thread whole. The
  # counter-factual: a photograph cut at a fixed length kept the threads the
  # sampler lists first. On macOS those were the JIT's threads, whose deep
  # stacks filled the cut before the wedged worker. The fixture parks 100
  # threads, which a cut sized for a few cannot hold.
  (eprintln "scenario: a timed-out form photographs every thread whole")
  (file/mkdir-all (path/join dir "photo"))
  (let [sleepers 100
        r (run-wedged "photo"
                      (string "(begin (each _ in (range 0 " sleepers ") "
                              "(os/spawn-vm (fn [] (time/sleep 600)))) "
                              "(ev/sleep 600))\n"))
        start (string/find r:said "── threads at the deadline")]
    (assert (string/contains? r:said "join: deadline exceeded")
            (string "the fixture must be recorded as a timeout: " r:said))
    (if (not (sampler?))
      (assert (nil? start)
              (string "a box with no sampler prints no photograph: " r:said))
      (begin
        (assert (not (nil? start))
                (string "a box with a sampler photographs a timeout: " r:said))
        (let [from (slice r:said start)
              end (string/find from "── end threads")
              photo (if (nil? end) from (slice from 0 end))]
          (assert (not (string/contains? photo "…truncated"))
                  (string "the photograph must be printed whole, "
                          (length photo) " bytes"))
          (assert (>= (thread-headers photo) sleepers)
                  (string "the photograph must list every thread: " sleepers
                          " parked, " (thread-headers photo) " listed"))))))

  (eprintln "all timeout-capture scenarios passed"))
