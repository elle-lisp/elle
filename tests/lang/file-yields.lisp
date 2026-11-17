(elle/epoch 15)
# audited: 2026-11-17
# The file metadata calls and the path probes yield to the scheduler, need :io and :fs, and let other fibers run.
# docs/io/files.md
#
# The counter-factual: a call that ran std::fs on the VM thread answered
# inside fiber/resume, so a mask of |:io| saw the answer rather than a
# request, and :deny |:io| let the call through.

(def root (file/mktempdir))
(def file (path/join root "file"))
(file/write file "data")

# What a call would change, were it to run. A caught request is never
# serviced and a denied call never runs, so each victim outlives the file.
(def victim (path/join root "victim"))
(file/write victim "keep")
(def empty-dir (path/join root "empty"))
(file/mkdir empty-dir)
(def tree (path/join root "tree"))
(file/mkdir-all (path/join tree "leaf"))

# One call of each, beside the name its denial reports.
(def calls
  [["file/stat" (fn [] (file/stat file))]
   ["file/lstat" (fn [] (file/lstat file))]
   ["file/size" (fn [] (file/size file))] ["file/ls" (fn [] (file/ls root))]
   ["file/mkdir" (fn [] (file/mkdir (path/join root "made")))]
   ["file/mkdir-all" (fn [] (file/mkdir-all (path/join root "made-all/a")))]
   ["file/delete" (fn [] (file/delete victim))]
   ["file/delete-dir" (fn [] (file/delete-dir empty-dir))]
   ["file/delete-dir-all" (fn [] (file/delete-dir-all tree))]
   ["file/rename" (fn [] (file/rename victim (path/join root "renamed")))]
   ["file/mktempdir" (fn [] (file/mktempdir))]
   ["path/exists?" (fn [] (path/exists? file))]
   ["path/file?" (fn [] (path/file? file))]
   ["path/dir?" (fn [] (path/dir? root))]
   ["path/canonicalize" (fn [] (path/canonicalize root))]])

(defn denial [call bit]
  "The payload a fiber that withholds `bit` stops on when it makes `call`."
  (let [f (fiber/new call |:error :io :fs| :deny bit)]
    (fiber/resume f)
    (assert (= (fiber/status f) :paused) "a denied call parks the fiber")
    (fiber/value f)))

(each [name call] in calls
  (let [f (fiber/new call |:io|)]
    (assert (io-request? (fiber/resume f))
            (string name " yields an io-request to a mask of |:io|")))
  (each bit in [|:io| |:fs|]
    (let [v (denial call bit)]
      (assert (= (get v :error) :capability-denied)
              (string name " under :deny " bit " is a capability denial"))
      (assert (= (get v :primitive) name) (string "the denial names " name))
      (assert (= (get v :denied) bit) (string "the denial names " bit)))))

(assert (= (file/read victim) "keep") "no delete or rename ran")
(assert (path/dir? empty-dir) "no delete-dir ran")
(assert (path/dir? (path/join tree "leaf")) "no delete-dir-all ran")
(assert (not (path/exists? (path/join root "made"))) "no mkdir ran")
(assert (not (path/exists? (path/join root "made-all"))) "no mkdir-all ran")

# ── Another fiber runs while one waits on the disk ─────────────────────

# `other` is runnable before the list starts. A list that ran on the VM
# thread finished first, and the log read [:listed :other].
(let [log @[]
      lister (ev/spawn (fn []
                         (let [other (ev/spawn (fn [] (push log :other)))]
                           (file/ls root)
                           (push log :listed)
                           (ev/join other))))]
  (ev/join lister)
  (assert (= (freeze log) [:other :listed])
          "another fiber runs while file/ls waits"))

# ── The answers, and the errors that name the path ─────────────────────

(let [missing (path/join root "missing")]
  (each call in [(fn [] (file/stat missing)) (fn [] (file/lstat missing))
                 (fn [] (file/size missing)) (fn [] (file/ls missing))
                 (fn [] (file/delete missing)) (fn [] (file/delete-dir missing))
                 (fn [] (file/delete-dir-all missing))
                 (fn [] (file/rename missing file))
                 (fn [] (path/canonicalize missing))]
    (let [[ok? err] (protect (call))]
      (assert (not ok?) "a missing path fails")
      (assert (= (get err :error) :io-error) "with an :io-error")
      (assert (= (get err :path) missing) "whose :path names it")))
  (assert (= false (path/exists? missing) (path/file? missing)
             (path/dir? missing)) "a probe of a missing path answers false"))

(assert (= false (path/exists? "a\0b")) "a probe never raises, even on a NUL")
(assert (= 4 (file/size file)))
(assert (= "file" (get (file/stat file) :file-type)))

# ── with-temp-dir's cleanup runs in a body an abort ends ───────────────

# The delete in the cleanup waits on the scheduler too, so the aborted fiber
# suspends inside its own unwinding, and the scheduler has to finish it. The
# abort returns first, so each case waits for the fiber's end.
(defn scratch-body [cell]
  "A body that makes a scratch directory, names it in `cell`, and waits."
  (fn []
    (with-temp-dir dir
      (put cell 0 dir)
      (file/write (path/join dir "f") "x")
      (ev/sleep 10))))

(let [cell @[nil]
      body (ev/spawn (scratch-body cell))]
  (while (nil? (get cell 0)) (ev/sleep 0.001))
  (ev/abort body)
  (let [[ok? err] (ev/join-protected body)]
    (assert (not ok?) "the abort ends the body")
    (assert (= (get err :error) :aborted) "with :aborted"))
  (assert (not (path/exists? (get cell 0))) "and its directory is gone"))

# ev/race aborts the loser as ev/timeout does. The winner waits for the
# directory, so the race ends with one on disk.
(let [cell @[nil]
      ready (fn []
              (while (nil? (get cell 0)) (ev/sleep 0.001))
              :ready)]
  (assert (= :ready (ev/race [(ev/spawn (scratch-body cell)) (ev/spawn ready)]))
          "the waiter wins")
  (while (path/exists? (get cell 0)) (ev/sleep 0.001))
  (assert (not (path/exists? (get cell 0)))
          "and the losing body's directory goes"))

(file/delete-dir-all root)
(println "file-yields: OK")
