(elle/epoch 13)
# audited: 2026-09-30
## elle test — the readings a result printed: read out of its captured stdout,
## judged against the file's ledger, recorded, and the rows nobody answered.
## docs/ratchet.md
##
## A fragment of one module (see store.lisp).

# The judge, the row reader and the line reader are the instrument's own, so
# the runner and a direct run of a producer reach one verdict from one function.
(def ledger ((import "std/ratchet/ledger")))

# Every ledger in the directory, keyed by producer, loaded once: a ledger is a
# committed file and a run reads it, never writes it. ELLE_LEDGER names another
# directory, which is how a test hands the runner a ledger of its own.
(def ledgers
  (let [dir (ledger:ledger-dir (elle/root))]
    (if (and dir (file/exists? dir)) (ledger:load-dir dir) @{})))

(defn rows-for [file]
  "The ledger rows of FILE keyed by `row-key`, or nil when no ledger names it.
   FILE is matched as the command line gave it, relative to the root when it
   sits under the root — the same path a direct run judges as. An ad-hoc form
   has no file and so no ledger."
  (let [p (ledger:producer-of [file] (elle/root))
        l (if p (get ledgers p) nil)]
    (if l (get l :rows) nil)))

(defn insert-reading [conn run-id result-id r]
  "One reading as a row. A reading that met no ledger carries NULL for the
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

# Every judged reading of the run, each with the file that printed it, kept as
# it lands: --repin reads a reading's class and floor here, which the table
# does not carry, and moves a pin to the worst of every tier's readings.
(def @repin-queue @[])

(defn unjudged [r]
  "READING with whatever verdict its printer gave it removed. A producer that
   judged itself against another ledger than this run's has no standing here."
  (put (put (put r :bound nil) :kind nil) :verdict nil))

# A judged reading carries its row's bound and kind and the verdict it earned;
# an unjudged one carries the reading alone. A missing row is written only
# against a pass: a failed result already says so, and a gated one skipped
# rather than fell silent (docs/test-store.md § Measurements).
(defn record-readings [conn run-id result-id file text status]
  "Every reading TEXT carries, as rows against RESULT-ID, judged against
   FILE's ledger when it has one. When STATUS is :pass and the ledger holds a
   row no reading answered, that row is recorded missing."
  (let [readings (ledger:readings-in (if text text ""))
        rows (rows-for file)]
    (if rows
      (begin
        (each r in (ledger:judge-all readings rows)
          (insert-reading conn run-id result-id r)
          (push repin-queue (put r :file file)))
        (when (= status :pass)
          (each row in (ledger:unread rows readings)
            (insert-missing conn run-id result-id row))))
      (each r in readings
        (insert-reading conn run-id result-id (unjudged r)))))
  nil)

(defn count-gating-readings [conn run-id]
  "How many of the run's readings gate: every verdict but ok, and never an
   unjudged one."
  (get (get (sqlite:query conn
                          "SELECT count(*) AS c FROM measurement WHERE run_id = ?1 AND verdict IS NOT NULL AND verdict != 'ok'"
                          [run-id]) 0) :c))
