(elle/epoch 12)
# Git module write-path tests (lib/git.lisp, FFI to libgit2).
#
# Complements tests/elle/git.lisp, which reads the *current* repo (open, head,
# log, …); this file exercises the write path — init, config, add, commit,
# status, branches, tags — against a throwaway repo in a scratch temp dir.
#
# The module does not (yet) export diff/diff-patch/show/add-all/checkout, so
# those surfaces are untested here.

# Gate the whole file on libgit2: if it can't load, re-raise as a loud :gated
# so `elle test` records a file-level SKIP with a reason (docs § Gating).
# Never (exit 0): under the runner that would kill the process mid-run.
(def _libgit2
  (let [r (protect (ffi/native "libgit2.so"))]
    (if (get r 0)
      true
      (error (struct :error :gated :reason "libgit2.so not installed")))))

(def git ((import "std/git")))

# The repo lives in a subdir of a fresh scratch temp dir; the whole tree
# is removed at the end.
(let [scratch (file/mktempdir)
      tmp (path/join scratch "repo")]
  (let [repo (git:init tmp)]
    (assert (string? (git:path repo)) "git:path returns string")
    (assert (string? (git:workdir repo)) "git:workdir returns string")
    (assert (not (git:bare? repo)) "not bare")
    (assert (= :clean (git:state repo)) "state is :clean")

    # Config — set before any commits so git:commit can read user identity
    (git:config-set repo "user.name" "Test User")
    (git:config-set repo "user.email" "test@example.com")
    (assert (= "Test User" (git:config-get repo "user.name")) "config roundtrip")
    (assert (nil? (git:config-get repo "no.such.key"))
            "config-get nil for missing")

    # HEAD on empty repo should signal
    (let [r (protect (git:head repo))]
      (assert (not (first r)) "head errors on empty repo"))

    # Resolve on empty repo should signal
    (let [r (protect (git:resolve repo "HEAD"))]
      (assert (not (first r)) "resolve errors on empty repo"))

    # -------------------------------------------------------------------------
    # Write a file and stage it
    # -------------------------------------------------------------------------
    (let [filepath (path/join tmp "hello.txt")]
      (spit filepath "hello\n"))

    # Status before staging
    (let [s (git:status repo)]
      (assert (= 1 (length s)) "one untracked file")
      (assert (= :new (get (first s) :workdir)) "workdir :new")
      (assert (nil? (get (first s) :index)) "index nil"))

    # Stage the file
    (git:add repo "hello.txt")
    (let [s (git:status repo)]
      (assert (= :new (get (first s) :index)) "index :new after add"))

    # -------------------------------------------------------------------------
    # First commit
    # -------------------------------------------------------------------------
    (let [oid (git:commit repo "initial commit")]
      (assert (string? oid) "commit returns oid string")

      # HEAD now resolves
      (let [head (git:head repo)]
        (assert (string? (get head :oid)) "head oid is string")
        (assert (get head :symbolic) "head is symbolic"))

      # commit-info
      (let [info (git:commit-info repo oid)]
        (assert (= oid (get info :oid)) "commit-info oid matches")
        (assert (string? (get info :message)) "commit-info message is string")
        (assert (= 0 (length (get info :parents)))
                "initial commit has no parents"))

      # git:log
      (let [log (git:log repo {:limit 5})]
        (assert (= 1 (length log)) "log has 1 commit")
        (assert (= oid (get (first log) :oid)) "log first oid matches"))

      # git:resolve
      (let [resolved (git:resolve repo "HEAD")]
        (assert (string? resolved) "resolve HEAD returns string")
        (assert (= 40 (string/size-of resolved)) "OID is 40 chars"))

      # -------------------------------------------------------------------------
      # Status after commit — clean
      # -------------------------------------------------------------------------
      (let [s (git:status repo)]
        (assert (= 0 (length s)) "status clean after commit"))

      # -------------------------------------------------------------------------
      # Branches
      # -------------------------------------------------------------------------
      (let [branches (git:branches repo :local)]
        (assert (= 1 (length branches)) "one local branch")
        (assert (string? (get (first branches) :name)) "branch name is string"))

      (let [branch-oid (git:branch-create repo "feature")]
        (assert (string? branch-oid) "branch-create returns oid")
        (assert (= 2 (length (git:branches repo :local))) "two branches now"))

      (git:branch-delete repo "feature")
      (assert (= 1 (length (git:branches repo :local))) "back to one branch")

      # -------------------------------------------------------------------------
      # Tags
      # -------------------------------------------------------------------------
      (let [tag-oid (git:tag-create repo "v0.1")]
        (assert (string? tag-oid) "tag-create returns oid"))
      (assert (= 1 (length (git:tags repo))) "one tag")
      (assert (= "v0.1" (first (git:tags repo))) "tag name is v0.1")
      (git:tag-delete repo "v0.1")
      (assert (= 0 (length (git:tags repo))) "no tags after delete")

      # Annotated tag
      (let [tag-oid (git:tag-create repo "v1.0" "HEAD" "Release 1.0")]
        (assert (string? tag-oid) "annotated tag-create returns oid"))
      (git:tag-delete repo "v1.0")

      # -------------------------------------------------------------------------
      # Staging with modification
      # -------------------------------------------------------------------------
      (let [filepath (path/join tmp "hello.txt")]
        (spit filepath "hello world\n"))

      (let [s (git:status repo)]
        (assert (= :modified (get (first s) :workdir)) "workdir :modified"))

      (git:add repo "hello.txt")
      (let [s (git:status repo)]
        (assert (= :modified (get (first s) :index)) "staged after add"))

      # Second commit
      (let [oid2 (git:commit repo "second commit")]
        (assert (string? oid2) "second commit oid")
        (let [log (git:log repo {:limit 10})]
          (assert (= 2 (length log)) "log has 2 commits")))

      # -------------------------------------------------------------------------
      # Config (additional coverage)
      # -------------------------------------------------------------------------
      (git:config-set repo "core.autocrlf" "false")
      (assert (= "false" (git:config-get repo "core.autocrlf"))
              "config-set/get roundtrip")

      # -------------------------------------------------------------------------
      # Remotes (basic, no network)
      # -------------------------------------------------------------------------
      (let [remote-list (git:remotes repo)]
        (assert (= 0 (length remote-list)) "no remotes in fresh repo")))

    (git:close repo))

  # Cleanup
  (file/delete-dir-all scratch)
  (println "git tests passed"))
