(elle/epoch 14)
# audited: 2026-10-05
# The audit queue's two counts, read against the rows of
# tests/ledger/audit.lisp: the files with no stamp, and the files stamped
# before the policy.
# docs/impl/audit.md
#
# The script walks the files git tracks under the working directory, which is
# the repository root, as it is for every path the runner takes.

(def r ((import "std/ratchet")))

(def counts (subprocess/system "scripts/audit" ["--counts"]))
(assert (= (get counts :exit) 0)
        (string "scripts/audit --counts exits 0: " (get counts :stderr)))

(def lines (string/split (get counts :stdout) "\n"))

(defn count-of [name]
  "The count scripts/audit printed on its line `NAME N`."
  (let [prefix (string name " ")
        found (filter (fn [l] (string/starts-with? l prefix)) lines)]
    (assert (= (length found) 1) (string "one line names " name))
    (let [line (first found)]
      (parse-int (slice line (length prefix) (length line))))))

(r:read "unstamped files" :files (count-of "unstamped"))
(r:read "files stamped before the policy" :files (count-of "before-policy"))
