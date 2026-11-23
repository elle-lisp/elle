(elle/epoch 18)
# audited: 2026-11-23
# The generated index's two counts, read against the rows of
# tests/ledger/agents.lisp: the hand-written indexes, and the rows in them that
# name a file.
# tools/agents/overview.md
#
# The generator's module reads the files git tracks under the working directory,
# which is the repository root, as it is for every path the runner takes.

(def r ((import "std/ratchet")))
(def agents ((import "../../tools/agents/agents")))

(def counts (agents:counts (path/cwd)))

(r:read "hand-written indexes" :files (get counts :hand-written))
(r:read "rows naming a file in a hand-written index"
        :rows (get counts :file-rows))
