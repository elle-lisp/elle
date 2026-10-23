(elle/epoch 15)
# audited: 2026-10-23
# The generated index's two counts, read against the rows of
# tests/ledger/agents.lisp: the hand-written indexes, and the rows in them that
# name a file.
# docs/impl/agents-index.md
#
# The script walks the files git tracks under the working directory, which is
# the repository root, as it is for every path the runner takes.

(def r ((import "std/ratchet")))

(def counts (subprocess/system "scripts/agents" ["--counts"]))
(assert (= (get counts :exit) 0)
        (string "scripts/agents --counts exits 0: " (get counts :stderr)))

(def lines (string/split (get counts :stdout) "\n"))

(defn count-of [name]
  "The count scripts/agents printed on its line `NAME N`."
  (let [prefix (string name " ")
        found (filter (fn [l] (string/starts-with? l prefix)) lines)]
    (assert (= (length found) 1) (string "one line names " name))
    (let [line (first found)]
      (parse-int (slice line (length prefix) (length line))))))

(r:read "hand-written indexes" :files (count-of "hand-written"))
(r:read "rows naming a file in a hand-written index"
        :rows (count-of "file-rows"))
