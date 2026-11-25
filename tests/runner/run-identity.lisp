(elle/epoch 18)
## audited: 2026-11-22
## A `run` row names the code it ran against, so a result belongs to something and a warning says whose run it warns about.
## tests/runner/overview.md
## docs/test-store.md
##
## The counter-factual: without these columns every row reads the same whatever
## commit produced it, a search for a regression has nothing to group by, and
## the killed-run warning fires on a sibling checkout's live run with no way to
## tell the reader that is what happened.
##
## The expected values come from the tools the runner reads, run here, so a box
## that spells its hostname differently still agrees with itself. The version
## comes from Cargo.toml, a source the runner does not read.

(include-file "lib/harness.lisp")

(def root (path/cwd))

(defn shell [cwd program args]
  "The trimmed stdout of PROGRAM ARGS run in CWD."
  (string/trim ((subprocess/system program args {:cwd cwd}) :stdout)))

(def head (shell root "git" ["rev-parse" "HEAD"]))
(def worktree (shell root "git" ["rev-parse" "--show-toplevel"]))

(defn last-run [db cols]
  "The columns COLS of the latest run's row in DB."
  (get (rows db (string "SELECT " cols " FROM run WHERE id = " latest-run)) 0))

(with-temp-dir dir
  (def passes (write-fixture dir "pass.lisp" "(assert true \"ok\")\n"))

  (defn run-in [cwd db]
    (elle-test db [passes] :cwd cwd))

  (eprintln "scenario: a run row carries the commit, the host and the build")
  (let [db (path/join dir "commit.db")
        r (run-in root db)
        version (get (string/split (first
                                     (filter
                                       (fn [l]
                                         (string/starts-with? l "version = "))
                                       (string/split (slurp "Cargo.toml") "\n")))
                                   "\"") 1)
        profile ((elle-run ["-e" "(print (elle/build-profile))"]) :out)
        row (last-run db
                      (string "git_commit AS sha, host, worktree, "
                              "elle_version AS version, "
                              "build_profile AS profile, "
                              "(length(tree_hash) > 0) AS hastree"))]
    (assert (= r:exit 0) (string "the run should gate green; stderr: " r:err))
    (each [what want] [[:sha head] [:host (shell root "uname" ["-n"])]
                       [:worktree worktree] [:version version]
                       [:profile profile]]
      (assert (= (get row what) want)
              (string "the run row must record " what " as " want ", got "
                      (get row what))))
    (assert (= row:hastree 1)
            (string "the run row must record a tree hash: " row)))

  # Outside a repository there is no code state to record. NULL says that; a
  # made-up value would say something false.
  (eprintln "scenario: a run outside a repository records no commit")
  (let [db (path/join dir "norepo.db")
        elsewhere (path/join dir "elsewhere")]
    (file/mkdir-all elsewhere)
    (let [r (run-in elsewhere db)
          row (last-run db
                        (string "(git_commit IS NULL) AS nocommit, "
                                "(worktree IS NULL) AS noworktree, "
                                "(length(host) > 0) AS hashost"))]
      (assert (= r:exit 0)
              (string "a run outside a repository still gates on its results; "
                      "stderr: " r:err))
      (assert (and (= row:nocommit 1) (= row:noworktree 1))
              (string "no repository means no commit and no worktree: " row))
      (assert (= row:hashost 1)
              "the host is a fact about the machine, not the checkout")))

  # A tally in a log says nothing until it says which code it describes.
  (eprintln "scenario: the summary names the commit")
  (let [db (path/join dir "summary.db")]
    (run-in root db)
    (let [s (elle-test db ["--summary"])]
      (assert (string/contains? s:err (slice head 0 7))
              (string "--summary must name the run's commit, got: " s:err))))

  # One session DB serves every checkout on the box, so the warning has to say
  # whose run it found unfinished. A sibling worktree running its own corpus
  # leaves the same row a kill does.
  (eprintln "scenario: the kill warning names the worktree")
  (let [db (path/join dir "killed.db")
        killed (elle-test db
                          [(write-fixture dir "kills.lisp"
                             "(os/sig-raise :sigkill)\n")] :cwd root)]
    (assert (= killed:exit -9)
            (string "the fixture must SIGKILL the runner; stderr: " killed:err))
    (let [next (run-in root db)]
      (assert (string/contains? next:err "was killed")
              (string "the next run must warn about its killed predecessor: "
                      next:err))
      (assert (string/contains? next:err worktree)
              (string "the warning must name the worktree " worktree ": "
                      next:err))))

  (eprintln "all run-identity scenarios passed"))
