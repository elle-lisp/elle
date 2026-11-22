(elle/epoch 18)
## audited: 2026-11-22
## Starting the binary in a directory a module test picks, with the search set by its flags alone.
## tests/cli/overview.md
## docs/modules/resolution.md
##
## Splice this after the runner's harness, whose elle-feed and env-without it
## reads.

# ELLE_PATH and ELLE_HOME are cleared, so the flags alone decide where a spec
# resolves.
(def module-env (env-without "ELLE_PATH" "ELLE_HOME"))

(defn run-in [cwd args input]
  "Run the binary in CWD with ARGS and INPUT on its standard input."
  (elle-feed args input :env module-env :cwd cwd))

(defn write-module [root rel value]
  "Write VALUE as the module at REL under ROOT, with the directories REL
   names, and answer its path."
  (let [path (path/join root rel)]
    (file/mkdir-all (path/parent path))
    (file/write path (string value "\n"))
    path))
