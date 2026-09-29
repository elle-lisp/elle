(elle/epoch 13)
# audited: 2026-09-29
# Runs an elle under callgrind and prints the code paths that claim the most region pages.
# docs/impl/region/colocation.md
#
#   elle tools/pageclaims/run.lisp -- [--depth N] [--top N] [--elle PATH] ELLE-ARGS…
#
# `make page-claims ARGS=…` builds the profiling profile and runs this from the
# repository root, which the imports below resolve against. The callgrind file
# and the stdlib cache live in a temporary directory that the run deletes.

(def {:rank rank} ((import-file "tools/pageclaims/rank.lisp")))
(def {:options options :commands commands}
  ((import-file "tools/pageclaims/options.lisp")))

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

(with-temp-dir dir (profile dir))
