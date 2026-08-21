(elle/epoch 12)
## Git module tests (FFI to libgit2)

# Gate the whole file on libgit2: if it can't load, re-raise as a loud :gated so
# `elle test` records a file-level SKIP with a reason (docs § Gating). Eager
# (def …), so it gates during barrier-module setup, before any test thunk.
# Never (exit 0): under the runner that would kill the process mid-run.
(def _libgit2
  (let [r (protect (ffi/native "libgit2.so"))]
    (if (get r 0)
      true
      (error (struct :error :gated :reason "libgit2.so not installed")))))

(def git ((import "std/git")))

## Open current repo
(def repo (git:open "."))

## Head
(let [h (git:head repo)]
  (assert (string? h:name) "head name is string")
  (assert (string? h:oid) "head oid is string")
  (assert (= (length h:oid) 40) "oid is 40 hex chars"))

## Resolve
(let* [oid (git:resolve repo "HEAD")
       h (git:head repo)]
  (assert (= oid h:oid) "resolve HEAD matches head oid"))

## Commit info
(let* [oid (git:resolve repo "HEAD")
       info (git:commit-info repo oid)]
  (assert (= info:oid oid) "commit-info oid matches")
  (assert (string? info:summary) "has summary")
  (assert (string? info:author:name) "has author name")
  (assert (string? info:author:email) "has author email")
  (assert (integer? info:author:time) "has author time")
  (assert (list? info:parents) "has parents list"))

## Log
(let [entries (git:log repo {:limit 3})]
  (assert (> (length entries) 0) "log has entries")
  (assert (<= (length entries) 3) "log respects limit")
  (let [e (first entries)]
    (assert (string? e:summary) "log entry has summary")))

## Branches
(let [bs (git:branches repo :local)]
  (assert (list? bs) "branches returns list")
  (when (> (length bs) 0)
    (let [b (first bs)]
      (assert (string? b:name) "branch has name"))))

## Tags
(assert (list? (git:tags repo)) "tags returns list")

## Remotes
(assert (list? (git:remotes repo)) "remotes returns list")

## Path and state
(assert (string? (git:path repo)) "path is string")
(assert (= (git:state repo) :clean) "state is clean")
(assert (not (git:bare? repo)) "not bare")
(assert (string? (git:workdir repo)) "workdir is string")

## Init a temp repo, branch, tag, commit
## (scratch is defined after the libgit2 gate above, so a gated run never
## creates — and thus never leaks — a temp dir; removed at the bottom.)
(def scratch (file/mktempdir))
(def tmp-path (path/join scratch "repo"))
(def tmp (git:init tmp-path))
(git:config-set tmp "user.name" "Test")
(git:config-set tmp "user.email" "test@test.com")
(assert (= (git:config-get tmp "user.name") "Test") "config roundtrip")

## Create a file, add, commit
(file/write (string tmp-path "/README.md") "hello")
(git:add tmp "README.md")
(def oid (git:commit tmp "initial commit"))
(assert (= (length oid) 40) "commit returns oid")

## Branch
(def branch-oid (git:branch-create tmp "feature"))
(assert (= branch-oid oid) "branch points to HEAD")
(let [bs (git:branches tmp :local)]
  (assert (>= (length bs) 2) "two branches"))
(git:branch-delete tmp "feature")

## Tag
(git:tag-create tmp "v1.0")
(let [ts (git:tags tmp)]
  (assert (find (fn [t] (= t "v1.0")) ts) "tag created"))
(git:tag-delete tmp "v1.0")

(git:close tmp)
(file/delete-dir-all scratch)

(git:close repo)

# Optional graceful teardown of libgit2's global state. No longer REQUIRED for
# safety: the library mapping is process-global and never `dlclose`d, so this file —
# which under `elle test` runs as a whole-file thunk in an os/spawn worker — exits
# cleanly regardless of any thread-local destructor libgit2 registered. See
# lib/git.lisp § shutdown.
(git:shutdown)
(println "git: all tests passed")
