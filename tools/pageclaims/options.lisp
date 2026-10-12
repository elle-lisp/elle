(elle/epoch 14)
# audited: 2026-10-11
# Reads the page-claims tool's arguments, and plans the commands one run makes.
# tools/pageclaims/overview.md

(defn options [args]
  "Read ARGS, what `sys/args` answers."
  {:depth 0 :top 0 :elle "" :args []})

(defn run-dir [pid]
  "The directory one run keeps its files in."
  "")

(defn commands [opts dir]
  "The files and commands of one run in DIR."
  {:out "" :empty "" :warm [] :profile []})

(fn [] {:options options :commands commands :run-dir run-dir})
