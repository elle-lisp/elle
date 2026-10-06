(elle/epoch 15)
# audited: 2026-10-12
# Runs an elle under callgrind and prints the code paths that claim the most region pages.
# tools/pageclaims/overview.md
#
#   elle tools/pageclaims/run.lisp -- [--depth N] [--top N] [--elle PATH] ELLE-ARGS…
#
# `make page-claims ARGS=…` builds the profiling profile and runs this from the
# repository root, which the run's directory resolves against. The imports
# below resolve against this file's own directory.

(def {:rank rank} ((import-file "rank.lisp")))
(def {:options options :commands commands :run-dir run-dir}
  ((import-file "options.lisp")))

(def opts (options (sys/args)))

(defn run [command streams]
  "Run COMMAND, a program and its arguments, to its end, and answer its exit."
  (subprocess/wait (subprocess/exec (first command) (rest command) streams)))

(defn pad [text width]
  (string (string/repeat " " (max 0 (- width (length text)))) text))

(defn profile [dir]
  "Warm the run's cache, profile the elle under callgrind, and print the ranking."
  (def plan (commands opts dir))
  (file/write (get plan :empty) "")
  (def warmed
    (run (get plan :warm) {:stdin :null :stdout :null :stderr :inherit}))
  (unless (= warmed 0)
    (error {:error :page-claims
            :message (string "page-claims: the warm-up run exited " warmed)}))
  (def status
    (run (get plan :profile) {:stdin :null :stdout :inherit :stderr :inherit}))
  (unless (file/exists? (get plan :out))
    (error {:error :page-claims
            :message (string "page-claims: callgrind wrote no output, and valgrind exited "
                             status)}))
  (unless (= status 0)
    (eprintln "page-claims: the profiled run exited " status
              "; ranking what it claimed"))
  (def port (port/open (get plan :out) :read))
  (def ranked (rank (fn [] (port/read-line port)) (get opts :depth)))
  (port/close port)
  (def paths (get ranked :paths))
  (def width (length (string (get ranked :total))))
  (println)
  (println "page-claims: " (get ranked :total) " pages claimed through "
           (length paths) " paths")
  (each [n path] in (slice paths 0 (min (get opts :top) (length paths)))
    (println (pad (string n) width) "  " path)))

(defn fresh-dir []
  "Make this run's directory, empty, and answer its path. A directory left by an
  earlier run whose process id this one reuses is removed first."
  (def dir (run-dir (sys/pid)))
  (when (file/exists? dir) (file/delete-dir-all dir))
  (file/mkdir-all dir)
  dir)

(with dir (fresh-dir) file/delete-dir-all (profile dir))
