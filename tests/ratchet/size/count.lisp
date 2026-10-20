(elle/epoch 15)
# audited: 2026-10-20
# The size producer's arithmetic: each kind's cap, the bucket a file reads at, and the excess split between the work and the tables.
# tests/ratchet/overview.md

(def source-cap 2500)
(def document-cap 4000)
(def bucket 500)

# The tables admitted over the cap (DOCUMENTATION.md), each entry a source's
# path and the rows it holds: {:path "…" :rows "…"}.
(def tables [])

(defn cap-of [path]
  "The cap of the file at PATH: a document's when it ends `.md`, as
   scripts/audit --documents reads it, and a source's otherwise."
  (if (string/ends-with? path ".md") document-cap source-cap))

(defn reading [path tokens]
  "The bucket the file at PATH reads at when it costs TOKENS, rounded up to a
   multiple of BUCKET, or nil at its cap or under it."
  (when (> tokens (cap-of path))
    (* bucket (/ (+ tokens (- bucket 1)) bucket))))

(defn excess [readings tables]
  "Each reading less its file's cap, summed, as {:sheds n :tables m}.
   READINGS is [[path value] …]; a path TABLES admits adds to :tables, and
   every other to :sheds."
  (let [admitted (set ;(map (fn [t] (get t :path)) tables))
        @sheds 0
        @carried 0]
    (each [path value] in readings
      (let [over (- value (cap-of path))]
        (if (contains? admitted path)
          (assign carried (+ carried over))
          (assign sheds (+ sheds over)))))
    {:sheds sheds :tables carried}))
