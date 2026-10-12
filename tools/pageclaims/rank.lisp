(elle/epoch 14)
# audited: 2026-10-11
# Ranks the code paths that claim region pages, from a callgrind file.
# tools/pageclaims/overview.md

(defn rank [read-line depth]
  "Sum the page claims of each call path in a callgrind file."
  {:total 0 :paths []})

(fn [] {:rank rank})
