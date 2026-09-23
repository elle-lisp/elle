(elle/epoch 12)
## audited: 2026-09-21
## lib/git.lisp — Git repository access via FFI to libgit2: the facade
## over lib/git/{core,read,write}.lisp.
##
## Usage:
##   (def git ((import "std/git")))
##   (def repo (git:open "."))
##   (println (git:head repo))
##   (each entry in (git:log repo {:limit 5})
##     (println entry:summary))
##   (git:close repo)

(fn []
  (def core ((import "std/git/core")))
  (def rd ((import "std/git/read") :core core))
  (def wr ((import "std/git/write") :core core))

  (def cfn core:cfn)
  (def check core:check)
  (def with-pp core:with-pp)

  (def c-repo-open (cfn "git_repository_open" :int @[:ptr :string]))
  (def c-repo-init (cfn "git_repository_init" :int @[:ptr :string :int]))
  (def c-clone (cfn "git_clone" :int @[:ptr :string :string :ptr]))
  (def c-repo-free (cfn "git_repository_free" :void @[:ptr]))
  (def c-repo-path (cfn "git_repository_path" :ptr @[:ptr]))
  (def c-repo-workdir (cfn "git_repository_workdir" :ptr @[:ptr]))
  (def c-repo-bare (cfn "git_repository_is_bare" :int @[:ptr]))
  (def c-repo-state (cfn "git_repository_state" :int @[:ptr]))

  ## ── Repository lifecycle ─────────────────────────────────────────

  (defn open [path]
    (let [repo (with-pp (fn [pp] (check (c-repo-open pp path) "git/open")))]
      repo))

  (defn init [path]
    (let [repo (with-pp (fn [pp] (check (c-repo-init pp path 0) "git/init")))]
      repo))

  (defn clone-repo [url path]
    (let [repo (with-pp (fn [pp]
                          (check (c-clone pp url path core:null-ptr) "git/clone")))]
      repo))

  (defn close [repo]
    (c-repo-free repo)
    nil)

  ## Optional graceful teardown for libgit2. The library mapping is process-global
  ## and never `dlclose`d (src/ffi/registry.rs), so a worker that uses git and then
  ## exits is safe regardless: its OpenSSL/libssh2 per-thread state is cleaned up by
  ## the thread-exit destructor running against still-mapped code. `git:shutdown` is
  ## therefore NO LONGER REQUIRED to avoid a crash — it is optional cleanup that
  ## brings libgit2's global refcount to zero (running the unload destructor
  ## registered at init via `ffi/run-teardowns`). Call it only when the threads that
  ## used git have quiesced, since `git_libgit2_shutdown` deletes the pthread key it
  ## created.
  (defn shutdown []
    (ffi/run-teardowns)
    nil)

  (defn repo-path [repo]
    (ffi/string (c-repo-path repo)))
  (defn workdir [repo]
    (core:maybe-str (c-repo-workdir repo)))
  (defn bare? [repo]
    (not (zero? (c-repo-bare repo))))

  (defn state [repo]
    (match (c-repo-state repo)
      0 :clean
      1 :merge
      2 :revert
      3 :revert-sequence
      4 :cherry-pick
      5 :cherry-pick-sequence
      6 :bisect
      7 :rebase
      8 :rebase-interactive
      9 :rebase-merge
      10 :apply-mailbox
      11 :apply-mailbox-or-rebase
      _ :unknown))

  {:open open
   :init init
   :clone clone-repo
   :close close
   :path repo-path
   :workdir workdir
   :bare? bare?
   :state state
   :head rd:head
   :resolve rd:resolve
   :commit-info rd:commit-info
   :log rd:log
   :status rd:status
   :branches rd:branches
   :tags rd:tags
   :remotes rd:remotes
   :remote-info rd:remote-info
   :config-get rd:config-get
   :show rd:show
   :ls-tree rd:ls-tree
   :commit wr:commit
   :add wr:add
   :remove wr:remove
   :branch-create wr:branch-create
   :branch-delete wr:branch-delete
   :tag-create wr:tag-create
   :tag-delete wr:tag-delete
   :config-set wr:config-set
   :fetch wr:fetch
   :shutdown shutdown})
