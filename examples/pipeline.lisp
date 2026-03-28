## examples/pipeline.lisp — a data pipeline with interesting signal topology
##
## Demonstrates the kind of program the living model was designed to reason
## about: mixed pure/impure phases, higher-order delegation, shared mutable
## state, and cross-boundary calls into Rust primitives.

# ── Pure core ──────────────────────────────────────────────────────────

(defn parse-record [raw]
  "Parse a colon-delimited record into a struct."
  (let [[parts (string/split raw ":")]]
    {:name (get parts 0)
     :value (get parts 1)
     :tag (get parts 2)}))

(defn validate [record]
  "Reject records with missing fields."
  (when (or (nil? (get record :name))
            (nil? (get record :value)))
    (error {:error :validation :message "missing required field"}))
  record)

(defn normalize [record]
  "Lowercase the name, trim the value."
  (put (put record
    :name (string/downcase (get record :name)))
    :value (string/trim (get record :value))))

(defn transform [record f]
  "Apply a user-supplied transformation to a record."
  (f record))

# ── Stateful accumulator ──────────────────────────────────────────────

(defn make-accumulator []
  "Create a mutable accumulator with a running count."
  (var count 0)
  (var items @[])
  {:add    (fn [item]
             (assign count (+ count 1))
             (push items item)
             count)
   :items  (fn [] (freeze items))
   :count  (fn [] count)})

# ── I/O boundary ──────────────────────────────────────────────────────

(defn read-records [port]
  "Read all lines from a port and parse each as a record."
  (var records @[])
  (var line (port/read-line port))
  (while (not (nil? line))
    (when (not (= line ""))
      (push records (parse-record line)))
    (assign line (port/read-line port)))
  (freeze records))

(defn write-report [port records]
  "Write a summary report to a port."
  (port/write port (string/format "Records: {}\n" (length records)))
  (each r in records
    (port/write port (string/format "  {} = {}\n" (get r :name) (get r :value))))
  (port/flush port))

# ── Pipeline ──────────────────────────────────────────────────────────

(defn process-pipeline [input-port output-port transform-fn]
  "End-to-end pipeline: read → validate → normalize → transform → report."
  (let* [[raw      (read-records input-port)]
         [valid    (filter (fn [r]
                     (let [[[ok? _] (protect (validate r))]]
                       ok?))
                     raw)]
         [normed   (map (fn [r] (normalize r)) valid)]
         [results  (map (fn [r] (transform r transform-fn)) normed)]]
    (write-report output-port results)
    results))
