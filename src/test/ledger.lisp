(elle/epoch 18)
# audited: 2026-11-24
## elle test — the readings a result printed, judged against the build's ledger
## rows and recorded, and the rows nobody read.
## docs/ratchet.md
##
## A fragment of one module (see store.lisp). `run-build` and `ledgers` are
## settings store.lisp declares and main.lisp assigns.

# The judge, the row reader and the line reader are the ledger module's: the
# runner is the one judge (docs/ratchet.md).
(def ledger ((import "std/ratchet/ledger")))

(defn rows-for [file]
  "FILE's rows of the run's build, keyed by `row-key`: an empty table when
   FILE's ledger holds none of them, and nil when no ledger names FILE. FILE
   is matched as the command line gave it, relative to the working directory
   when it is an absolute path under it. An ad-hoc form has no file and so no
   ledger."
  (let [l (get ledgers (ledger:producer-of file))]
    (if l (get l :rows) nil)))

(defn insert-reading [conn run-id result-id r]
  "One reading as a row. A reading that met no row carries NULL for the
   bound, the kind and the verdict: recorded, and not judged."
  (sqlite:exec conn
               "INSERT INTO measurement (run_id, result_id, subject, axis, value, half, unit, bound, kind, verdict) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"
               [run-id result-id (get r :subject) (get r :axis) (get r :value)
                (get r :half) (get r :unit) (get r :bound) (get r :kind)
                (get r :verdict)]))

(defn insert-missing [conn run-id result-id row]
  "One ledger row the result printed no reading for."
  (sqlite:exec conn
               "INSERT INTO measurement (run_id, result_id, subject, axis, bound, kind, verdict) VALUES (?1,?2,?3,?4,?5,?6,?7)"
               [run-id result-id (get row :subject) (get row :axis)
                (get row :bound) (get row :kind) :missing]))

# Every reading --repin may act on, each with the file that printed it, kept
# as it lands. A reading's class and floor are here and not in the table, and
# a pin moves to the worst of every tier's readings.
(def @repin-queue @[])

# How many readings a run with no build left unrecorded, for the summary's
# one line saying so (view.lisp).
(def @unrecorded-readings 0)

# A judged reading carries its row's bound and kind and the verdict it earned;
# an unjudged one carries the reading alone.
(defn record [conn run-id result-id producer readings rows owed]
  "READINGS of PRODUCER, as the run's build decides. ROWS are the producer's
   rows of the build, and nil when no ledger names it. With no build, none is
   recorded. With no ledger, each is recorded unjudged. With a ledger holding
   none of the build's rows, each is recorded unjudged and queued for
   adoption. With rows of the build, each is judged, recorded and queued, and
   a row of OWED that no reading answered is recorded missing. OWED is nil
   when nothing is owed, which allocates nothing inside a charge's window."
  (if (nil? run-build)
    (assign unrecorded-readings (+ unrecorded-readings (length readings)))
    (cond
      (nil? rows)
        (each r in readings
          (insert-reading conn run-id result-id r))
      (empty? (keys rows))
        (each r in readings
          (insert-reading conn run-id result-id r)
          (push repin-queue (put r :file producer)))
      true
        (begin
          (each r in (ledger:judge-all readings rows)
            (insert-reading conn run-id result-id r)
            (push repin-queue (put r :file producer)))
          (when owed
            (each row in (ledger:unread owed readings)
              (insert-missing conn run-id result-id row))))))
  nil)

# A result owes its file's rows only when it passed: a failed result already
# says so, and a gated one skipped rather than fell silent (docs/test-store.md).
(defn record-readings [conn run-id result-id file text status]
  "Every reading TEXT carries, for the producer FILE, recorded by `record`."
  (let [rows (rows-for file)]
    (record conn run-id result-id file (ledger:readings-in (if text text ""))
            rows (if (= status :pass) rows nil))))

# ── the runner's own readings: a file's charge and its residue ───────
# The runner is the producer of both, and the file is the subject. Each pass
# reads one axis, and owes a file the rows of that axis alone: a row of the
# other axis is the other pass's (docs/test-cost.md).
(def runner-producer "elle test")

(defn rows-about [rows subject axis]
  "The rows of ROWS whose subject is SUBJECT and whose axis is AXIS, keyed as
   ROWS keys them."
  (let [@out @{}]
    (when rows
      (each k in (keys rows)
        (let [row (get rows k)]
          (when (and (= (get row :subject) subject) (= (get row :axis) axis))
            (put out k row)))))
    out))

(defn latest-result-id [conn run-id]
  (get (get (sqlite:query conn
                          "SELECT max(id) AS id FROM result WHERE run_id = ?1"
                          [run-id]) 0) :id))

(defn record-charge [conn run-id file objects]
  "FILE's charge, the OBJECTS its second run left live on the runner's heap,
   as one reading of the producer `elle test` against the run's latest result.
   FILE owes its own rows alone: every other row is another file's."
  (let [rows (rows-for runner-producer)]
    (record conn run-id (latest-result-id conn run-id) runner-producer
            [@{:subject file
               :axis :objects
               :value objects
               :half 0
               :unit "objects"}] rows (rows-about rows file :objects))))

(defn record-residue [conn run-id file]
  "FILE's residue, the regions its isolated child's teardown left alive, as
   one reading of the producer `elle test` against the child's result. Only a
   child that passed reads or owes: a failure already gates, and a skip ran
   nothing the row describes. A passing child that printed no residue owes
   its row, which reads as missing."
  (let [result-id (latest-result-id conn run-id)
        res (get (sqlite:query conn
                               "SELECT status, residue_regions AS n FROM result WHERE id = ?1"
                               [result-id]) 0)
        n (get res :n)
        rows (rows-for runner-producer)]
    (when (= (get res :status) "pass")
      (record conn run-id result-id runner-producer
              (if (nil? n)
                []
                [@{:subject file :axis :residue :value n :half 0 :unit "regions"}])
              rows (rows-about rows file :residue)))))

(defn count-gating-readings [conn run-id]
  "How many of the run's readings gate: every verdict but ok, and never an
   unjudged one."
  (get (get (sqlite:query conn
                          "SELECT count(*) AS c FROM measurement WHERE run_id = ?1 AND verdict IS NOT NULL AND verdict != 'ok'"
                          [run-id]) 0) :c))
