(elle/epoch 12)
## audited: 2026-09-21
## lib/semver/arbitrate.lisp — run the previous release's tests against
## the worktree code and answer pass, fail, or unavailable.
##
## docs/semver.md owns the mechanics; tests/elle/semver-check.lisp pins
## them end to end.
##
## Usage:
##   (def arb ((import "std/semver/arbitrate")))
##   (arb:arbitrate {:base surface :module "std/x" :exe elle :worktree "."})
##     => {:status :pass|:fail|:unavailable :reason "..."}

(fn []
  (def glob ((import "std/glob")))

  (defn unavailable [reason]
    {:status :unavailable :reason reason})

  (defn baseline-rev [git repo leaf version base]
    "A revparse spec for the released code: the release tag, else the
     recorded commit; nil when neither resolves."
    (let [tag (string leaf "/v" version)
          [tag-ok? _] (protect (git:resolve repo tag))]
      (if tag-ok?
        tag
        (let [c (and (get base :released) ((base :released) :commit))]
          (if c
            (let [[ok? _] (protect (git:resolve repo c))]
              (if ok? c nil))
            nil)))))

  (defn materialize [git repo rev paths scratch]
    "Write each committed test into SCRATCH, keeping relative paths."
    (map (fn [p]
           (let [dst (path/join scratch p)]
             (file/mkdir-all (path/parent dst))
             (file/write dst (git:show repo (string rev ":" p)))
             dst)) paths))

  (defn run-old-tests [exe wt files scratch]
    "The child run: `elle test` over FILES with the worktree as cwd, so
     the old tests' imports resolve through the search path to the new
     code. No import is rewritten and no module root is overridden —
     the runner's own stdlib imports must keep resolving too."
    (let [args (concat (list "test" "--db" (path/join scratch "arbitrate.db"))
                       (->list files))]
      (subprocess/system exe (->array args) {:cwd wt})))

  (defn arbitrate [opts]
    "Resolve the baseline rev, materialize its recorded tests, and run
     them against the worktree at (opts :worktree)."
    (block :done
      (let [base (opts :base)
            exe (opts :exe)
            wt (path/absolute (or (get opts :worktree) "."))
            leaf (path/filename (opts :module))
            pattern (get base :tests)]
        (unless pattern
          (break :done (unavailable "the .surface records no tests")))
        (let [[gok? git] (protect ((import "std/git")))]
          (unless gok? (break :done (unavailable "libgit2 unavailable")))
          (let [[rok? repo] (protect (git:open wt))]
            (unless rok? (break :done (unavailable "not a git repository")))
            (let [rev (baseline-rev git repo leaf (base :version) base)]
              (unless rev
                (git:close repo)
                (break :done (unavailable "no baseline rev: no release tag, no resolvable recorded commit")))
              (let [[lok? paths] (protect (git:ls-tree repo rev))]
                (unless lok?
                  (git:close repo)
                  (break :done (unavailable "cannot list the baseline tree")))
                (let [tests (->list (filter (fn [p] (glob:match-path? pattern p))
                                    paths))]
                  (when (empty? tests)
                    (git:close repo)
                    (break :done (unavailable "no tests at the baseline rev")))
                  (let [scratch (file/mktempdir)
                        [mok? files] (protect (materialize git repo rev tests
                        scratch))]
                    (git:close repo)
                    (unless mok?
                      (file/delete-dir-all scratch)
                      (break :done (unavailable "cannot read the baseline tests")))
                    (let [r (run-old-tests exe wt (->list files) scratch)]
                      (file/delete-dir-all scratch)
                      (if (= (r :exit) 0)
                        {:status :pass :count (length (->list files))}
                        {:status :fail
                         :reason "previous release's tests fail against the worktree"})))))))))))

  {:arbitrate arbitrate})
