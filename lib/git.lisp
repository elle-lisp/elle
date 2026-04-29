(elle/epoch 9)
## lib/git.lisp — Git repository access via FFI to libgit2
##
## Usage:
##   (def git ((import "std/git")))
##   (def repo (git:open "."))
##   (println (git:head repo))
##   (each entry in (git:log repo {:limit 5})
##     (println entry:summary))
##   (git:close repo)

(fn []
  (def lib (ffi/native "libgit2.so"))
  (def null-ptr (ptr/from-int 0))

  (defn cfn [name ret args]
    (let [p (ffi/lookup lib name)
          s (ffi/signature ret args)]
      (fn [& a] (apply ffi/call p s a))))

  ## Initialize libgit2
  ((cfn "git_libgit2_init" :int @[]))

  ## ── C bindings ───────────────────────────────────────────────────

  (def c-repo-open (cfn "git_repository_open" :int @[:ptr :string]))
  (def c-repo-init (cfn "git_repository_init" :int @[:ptr :string :int]))
  (def c-clone (cfn "git_clone" :int @[:ptr :string :string :ptr]))
  (def c-repo-free (cfn "git_repository_free" :void @[:ptr]))
  (def c-repo-path (cfn "git_repository_path" :ptr @[:ptr]))
  (def c-repo-workdir (cfn "git_repository_workdir" :ptr @[:ptr]))
  (def c-repo-bare (cfn "git_repository_is_bare" :int @[:ptr]))
  (def c-repo-state (cfn "git_repository_state" :int @[:ptr]))
  (def c-repo-head (cfn "git_repository_head" :int @[:ptr :ptr]))
  (def c-repo-index (cfn "git_repository_index" :int @[:ptr :ptr]))
  (def c-repo-config (cfn "git_repository_config" :int @[:ptr :ptr]))

  (def c-ref-name (cfn "git_reference_name" :ptr @[:ptr]))
  (def c-ref-target (cfn "git_reference_target" :ptr @[:ptr]))
  (def c-ref-free (cfn "git_reference_free" :void @[:ptr]))
  (def c-ref-is-branch (cfn "git_reference_is_branch" :int @[:ptr]))

  (def c-revparse (cfn "git_revparse_single" :int @[:ptr :ptr :string]))
  (def c-object-id (cfn "git_object_id" :ptr @[:ptr]))
  (def c-object-free (cfn "git_object_free" :void @[:ptr]))
  (def c-oid-tostr (cfn "git_oid_tostr_s" :ptr @[:ptr]))
  (def c-oid-fromstr (cfn "git_oid_fromstr" :int @[:ptr :string]))

  (def c-commit-lookup (cfn "git_commit_lookup" :int @[:ptr :ptr :ptr]))
  (def c-commit-message (cfn "git_commit_message" :ptr @[:ptr]))
  (def c-commit-summary (cfn "git_commit_summary" :ptr @[:ptr]))
  (def c-commit-author (cfn "git_commit_author" :ptr @[:ptr]))
  (def c-commit-committer (cfn "git_commit_committer" :ptr @[:ptr]))
  (def c-commit-parentcount (cfn "git_commit_parentcount" :int @[:ptr]))
  (def c-commit-parent-id (cfn "git_commit_parent_id" :ptr @[:ptr :int]))
  (def c-commit-tree-id (cfn "git_commit_tree_id" :ptr @[:ptr]))
  (def c-commit-free (cfn "git_commit_free" :void @[:ptr]))

  ## git_signature: name at offset 0 (ptr), email at offset 8 (ptr), when at offset 16
  ## when is git_time: time (i64) at +0, offset (int) at +8
  (def c-sig-now (cfn "git_signature_now" :int @[:ptr :string :string]))
  (def c-sig-free (cfn "git_signature_free" :void @[:ptr]))

  (def c-revwalk-new (cfn "git_revwalk_new" :int @[:ptr :ptr]))
  (def c-revwalk-push (cfn "git_revwalk_push" :int @[:ptr :ptr]))
  (def c-revwalk-push-head (cfn "git_revwalk_push_head" :int @[:ptr]))
  (def c-revwalk-sorting (cfn "git_revwalk_sorting" :void @[:ptr :int]))
  (def c-revwalk-next (cfn "git_revwalk_next" :int @[:ptr :ptr]))
  (def c-revwalk-free (cfn "git_revwalk_free" :void @[:ptr]))

  (def c-index-add-bypath (cfn "git_index_add_bypath" :int @[:ptr :string]))
  (def c-index-remove-bypath
    (cfn "git_index_remove_bypath" :int @[:ptr :string]))
  (def c-index-add-all
    (cfn "git_index_add_all" :int @[:ptr :ptr :int :ptr :ptr]))
  (def c-index-update-all
    (cfn "git_index_update_all" :int @[:ptr :ptr :ptr :ptr]))
  (def c-index-write (cfn "git_index_write" :int @[:ptr]))
  (def c-index-write-tree (cfn "git_index_write_tree" :int @[:ptr :ptr]))
  (def c-index-free (cfn "git_index_free" :void @[:ptr]))

  (def c-tree-lookup (cfn "git_tree_lookup" :int @[:ptr :ptr :ptr]))
  (def c-tree-free (cfn "git_tree_free" :void @[:ptr]))

  (def c-commit-create
    (cfn "git_commit_create"
         :int @[:ptr :ptr :string :ptr :ptr :ptr :string :ptr :size :ptr]))

  (def c-status-list-new (cfn "git_status_list_new" :int @[:ptr :ptr :ptr]))
  (def c-status-list-entrycount (cfn "git_status_list_entrycount" :size @[:ptr]))
  (def c-status-byindex (cfn "git_status_byindex" :ptr @[:ptr :size]))
  (def c-status-list-free (cfn "git_status_list_free" :void @[:ptr]))

  (def c-branch-iterator-new
    (cfn "git_branch_iterator_new" :int @[:ptr :ptr :int]))
  (def c-branch-next (cfn "git_branch_next" :int @[:ptr :ptr :ptr]))
  (def c-branch-iterator-free (cfn "git_branch_iterator_free" :void @[:ptr]))
  (def c-branch-name (cfn "git_branch_name" :int @[:ptr :ptr]))
  (def c-branch-create
    (cfn "git_branch_create" :int @[:ptr :ptr :string :ptr :int]))
  (def c-branch-delete (cfn "git_branch_delete" :int @[:ptr]))
  (def c-branch-lookup (cfn "git_branch_lookup" :int @[:ptr :ptr :string :int]))

  (def c-tag-list (cfn "git_tag_list" :int @[:ptr :ptr]))
  (def c-tag-create-lightweight
    (cfn "git_tag_create_lightweight" :int @[:ptr :ptr :string :ptr :int]))
  (def c-tag-create
    (cfn "git_tag_create" :int @[:ptr :ptr :string :ptr :ptr :string :int]))
  (def c-tag-delete (cfn "git_tag_delete" :int @[:ptr :string]))
  (def c-strarray-free (cfn "git_strarray_free" :void @[:ptr]))

  (def c-remote-list (cfn "git_remote_list" :int @[:ptr :ptr]))
  (def c-remote-lookup (cfn "git_remote_lookup" :int @[:ptr :ptr :string]))
  (def c-remote-url (cfn "git_remote_url" :ptr @[:ptr]))
  (def c-remote-pushurl (cfn "git_remote_pushurl" :ptr @[:ptr]))
  (def c-remote-fetch (cfn "git_remote_fetch" :int @[:ptr :ptr :ptr :ptr]))
  (def c-remote-push (cfn "git_remote_push" :int @[:ptr :ptr :ptr]))
  (def c-remote-free (cfn "git_remote_free" :void @[:ptr]))

  (def c-config-get-string
    (cfn "git_config_get_string" :int @[:ptr :ptr :string]))
  (def c-config-set-string
    (cfn "git_config_set_string" :int @[:ptr :string :string]))
  (def c-config-snapshot (cfn "git_config_snapshot" :int @[:ptr :ptr]))
  (def c-config-free (cfn "git_config_free" :void @[:ptr]))

  (def c-checkout-head (cfn "git_checkout_head" :int @[:ptr :ptr]))
  (def c-set-head (cfn "git_repository_set_head" :int @[:ptr :string]))
  (def c-set-head-detached
    (cfn "git_repository_set_head_detached" :int @[:ptr :ptr]))

  (def c-error-last (cfn "git_error_last" :ptr @[]))

  ## OID size: 20 bytes
  (def GIT_OID_SIZE 20)
  (def GIT_SORT_TIME 1)
  (def GIT_SORT_TOPOLOGICAL 2)
  (def GIT_ITEROVER -31)
  (def GIT_BRANCH_LOCAL 1)
  (def GIT_BRANCH_REMOTE 2)
  (def GIT_BRANCH_ALL 3)

  ## ── Helpers ──────────────────────────────────────────────────────

  (defn check [rc ctx]
    (unless (zero? rc)
      (let [err-ptr (c-error-last)]
        (if (= err-ptr null-ptr)
          (error {:error :git-error :message (string ctx ": error code " rc)})
          (error {:error :git-error
                  :message (string ctx ": " (ffi/string (ffi/read err-ptr :ptr)))})))))

  (defn with-pp [f]
    "Allocate a pointer-sized out-param, call f with it, return the read pointer."
    (let* [pp (ffi/malloc 8)
           result (f pp)
           ptr (ffi/read pp :ptr)]
      (ffi/free pp)
      ptr))

  (defn oid->str [oid-ptr]
    (ffi/string (c-oid-tostr oid-ptr)))

  (defn maybe-str [ptr]
    (if (= ptr null-ptr) nil (ffi/string ptr)))

  (defn sig->struct [sig-ptr]
    "Read a git_signature* into {:name :email :time}."
    {:name (ffi/string (ffi/read sig-ptr :ptr))
     :email (ffi/string (ffi/read (ptr/add sig-ptr 8) :ptr))
     :time (ffi/read (ptr/add sig-ptr 16) :i64)})

  (defn commit->struct [repo-ptr commit-ptr]
    "Read a git_commit* into a struct."
    (let* [nparents (c-commit-parentcount commit-ptr)
           parents (map (fn [i] (oid->str (c-commit-parent-id commit-ptr i)))
                        (->list (range nparents)))]
      {:oid (oid->str (c-object-id commit-ptr))
       :message (maybe-str (c-commit-message commit-ptr))
       :summary (maybe-str (c-commit-summary commit-ptr))
       :author (sig->struct (c-commit-author commit-ptr))
       :committer (sig->struct (c-commit-committer commit-ptr))
       :parents parents
       :tree (oid->str (c-commit-tree-id commit-ptr))}))

  ## ── Repository lifecycle ─────────────────────────────────────────

  (defn open [path]
    (let [repo (with-pp (fn [pp] (check (c-repo-open pp path) "git/open")))]
      repo))

  (defn init [path]
    (let [repo (with-pp (fn [pp] (check (c-repo-init pp path 0) "git/init")))]
      repo))

  (defn clone-repo [url path]
    (let [repo (with-pp (fn [pp]
                          (check (c-clone pp url path null-ptr) "git/clone")))]
      repo))

  (defn close [repo]
    (c-repo-free repo)
    nil)

  (defn repo-path [repo]
    (ffi/string (c-repo-path repo)))
  (defn workdir [repo]
    (maybe-str (c-repo-workdir repo)))
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

  (defn head [repo]
    (let* [ref-ptr (with-pp (fn [pp] (check (c-repo-head pp repo) "git/head")))
           result {:name (maybe-str (c-ref-name ref-ptr))
                   :oid (let [t (c-ref-target ref-ptr)]
                          (if (= t null-ptr) nil (oid->str t)))
                   :symbolic (not (zero? (c-ref-is-branch ref-ptr)))}]
      (c-ref-free ref-ptr)
      result))

  (defn resolve [repo refname]
    (let* [obj (with-pp (fn [pp]
                          (check (c-revparse pp repo refname) "git/resolve")))
           oid (oid->str (c-object-id obj))]
      (c-object-free obj)
      oid))

  ## ── Commits ──────────────────────────────────────────────────────

  (defn commit-info [repo oid-str]
    (let* [oid-buf (ffi/malloc GIT_OID_SIZE)
           _ (check (c-oid-fromstr oid-buf oid-str) "git/commit-info")
           commit (with-pp (fn [pp]
                             (check (c-commit-lookup pp repo oid-buf)
                                    "git/commit-info")))
           result (commit->struct repo commit)]
      (c-commit-free commit)
      (ffi/free oid-buf)
      result))

  (defn log [repo & opts]
    (let* [opt (if (> (length opts) 0) (first opts) {})
           from-ref (or opt:from nil)
           limit (or opt:limit 50)
           walker (with-pp (fn [pp] (check (c-revwalk-new pp repo) "git/log")))]
      (c-revwalk-sorting walker (bit/or GIT_SORT_TIME GIT_SORT_TOPOLOGICAL))
      (if from-ref
        (let [obj (with-pp (fn [pp]
                             (check (c-revparse pp repo from-ref) "git/log")))]
          (check (c-revwalk-push walker (c-object-id obj)) "git/log")
          (c-object-free obj))
        (check (c-revwalk-push-head walker) "git/log"))
      (let* [oid-buf (ffi/malloc GIT_OID_SIZE)
             results @[]]
        (def @i 0)
        (def @done false)
        (while (and (not done) (< i limit))
          (let [rc (c-revwalk-next oid-buf walker)]
            (if (not (zero? rc))
              (assign done true)
              (let* [commit (with-pp (fn [pp]
                                       (check (c-commit-lookup pp repo oid-buf)
                                       "git/log")))
                     entry (commit->struct repo commit)]
                (c-commit-free commit)
                (push results entry)
                (assign i (inc i))))))
        (c-revwalk-free walker)
        (ffi/free oid-buf)
        (->list results))))

  (defn commit [repo message & opts]
    (let* [opt (if (> (length opts) 0) (first opts) {})  ## Get index and write tree
           index (with-pp (fn [pp] (check (c-repo-index pp repo) "git/commit")))
           tree-oid (ffi/malloc GIT_OID_SIZE)
           _ (check (c-index-write-tree tree-oid index) "git/commit")
           tree (with-pp (fn [pp]
                           (check (c-tree-lookup pp repo tree-oid) "git/commit")))

           ## Get parent (HEAD commit, if any)
           parent-ref-pp (ffi/malloc 8)
           has-parent (zero? (c-repo-head parent-ref-pp repo))
           parent-commit (if has-parent
                           (let* [head-ref (ffi/read parent-ref-pp :ptr)
                                  head-oid (c-ref-target head-ref)
                                  pc (with-pp (fn [pp]
                                    (check (c-commit-lookup pp repo head-oid)
                                    "git/commit")))]
                             (c-ref-free head-ref)
                             pc)
                           nil)  ## Author/committer signatures
           author-name (or (and opt:author opt:author:name) nil)
           author-email (or (and opt:author opt:author:email) nil)
           committer-name (or (and opt:committer opt:committer:name) nil)
           committer-email (or (and opt:committer opt:committer:email) nil)  ## If no explicit name/email, read from config
           config (with-pp (fn [pp] (check (c-repo-config pp repo) "git/commit")))
           snap (with-pp (fn [pp]
                           (check (c-config-snapshot pp config) "git/commit")))
           cfg-name-pp (ffi/malloc 8)
           cfg-email-pp (ffi/malloc 8)
           _ (c-config-get-string cfg-name-pp snap "user.name")
           _ (c-config-get-string cfg-email-pp snap "user.email")
           cfg-name (maybe-str (ffi/read cfg-name-pp :ptr))
           cfg-email (maybe-str (ffi/read cfg-email-pp :ptr))
           a-name (or author-name cfg-name "Unknown")
           a-email (or author-email cfg-email "unknown@unknown")
           c-name (or committer-name cfg-name "Unknown")
           c-email (or committer-email cfg-email "unknown@unknown")
           author-sig (with-pp (fn [pp]
                                 (check (c-sig-now pp a-name a-email)
                                        "git/commit")))
           committer-sig (with-pp (fn [pp]
                                    (check (c-sig-now pp c-name c-email)
                                    "git/commit")))  ## Create commit
           new-oid (ffi/malloc GIT_OID_SIZE)
           parents-arr (if parent-commit
                         (let [pa (ffi/malloc 8)]
                           (ffi/write pa :ptr parent-commit)
                           pa)
                         null-ptr)
           nparents (if parent-commit 1 0)
           rc (c-commit-create new-oid repo "HEAD" author-sig committer-sig
                               null-ptr message tree nparents parents-arr)
           _ (check rc "git/commit")
           result (oid->str new-oid)]
      (when parent-commit
        (c-commit-free parent-commit)
        (ffi/free parents-arr))
      (c-sig-free author-sig)
      (c-sig-free committer-sig)
      (c-tree-free tree)
      (c-index-free index)
      (c-config-free snap)
      (c-config-free config)
      (ffi/free tree-oid)
      (ffi/free parent-ref-pp)
      (ffi/free cfg-name-pp)
      (ffi/free cfg-email-pp)
      (ffi/free new-oid)
      result))

  ## ── Status and staging ───────────────────────────────────────────

  (defn status-keyword [flags index?]
    "Convert git status bits to a keyword."
    (let [check (fn [bit kw] (when (not (zero? (bit/and flags bit))) kw))]
      (if index?
        (or (check 1 :new) (check 2 :modified) (check 4 :deleted)
            (check 8 :renamed) (check 16 :typechange) nil)
        (or (check 128 :new) (check 256 :modified) (check 512 :deleted)
            (check 1024 :renamed) (check 2048 :typechange) nil))))

  (defn status [repo]
    (let* [slist (with-pp (fn [pp]
                            (check (c-status-list-new pp repo null-ptr)
                                   "git/status")))
           count (c-status-list-entrycount slist)
           results @[]]
      (each i in (range count)  ## git_status_entry: status (u32 at 0),
        ## head_to_index (ptr at 8), index_to_workdir (ptr at 16)
        (let* [entry (c-status-byindex slist i)
               flags (ffi/read entry :u32)

               ## diff_delta has old_file.path at offset 8
               ## (after flags u32 + similarity u16 + nfiles u16).
               ## Just read from head_to_index or index_to_workdir delta.
               h2i (ffi/read (ptr/add entry 8) :ptr)
               i2w (ffi/read (ptr/add entry 16) :ptr)

               ## git_diff_delta layout:
               ##   status(u32,4) + flags(u32,4) +
               ##   similarity(u16,2) + nfiles(u16,2) = 12
               ## old_file starts at 16 (aligned).
               ##
               ## git_diff_file layout:
               ##   oid(20) + path(ptr,8) + size(i64,8) +
               ##   flags(u32,4) + mode(u16,2) + id_abbrev(u16,2)
               ##   = 44 -> padded 48
               ##
               ## oid is struct { unsigned char id[20]; }
               ## so old_file.path is at offset 16+20+4(pad)=40.
               ## This is fragile.
               ## Better: just get the path from whichever delta is non-null
               path-delta (if (not (= h2i null-ptr))
                            h2i
                            (if (not (= i2w null-ptr)) i2w null-ptr))]
          (when (not (= path-delta null-ptr))  ## Read path: the new_file.path is simpler to get. git_diff_delta layout varies by version.
            ## Safest approach: we know the entry has a path, just skip struct details for now.
            ## TODO: properly decode git_diff_delta struct offsets
            (push results
                  {:path ""
                   :index (status-keyword flags true)
                   :workdir (status-keyword flags false)}))))
      (c-status-list-free slist)
      (->list results)))

  (defn add [repo paths]
    (let* [index (with-pp (fn [pp] (check (c-repo-index pp repo) "git/add")))
           path-list (if (string? paths) (list paths) (->list paths))]
      (each p in path-list
        (check (c-index-add-bypath index p) "git/add"))
      (check (c-index-write index) "git/add")
      (c-index-free index)
      nil))

  (defn remove [repo paths]
    (let* [index (with-pp (fn [pp] (check (c-repo-index pp repo) "git/remove")))
           path-list (if (string? paths) (list paths) (->list paths))]
      (each p in path-list
        (check (c-index-remove-bypath index p) "git/remove"))
      (check (c-index-write index) "git/remove")
      (c-index-free index)
      nil))

  ## ── Branches ─────────────────────────────────────────────────────

  (defn branches [repo & opts]
    (let* [filter (if (> (length opts) 0)
                    (match (first opts)
                      :local GIT_BRANCH_LOCAL
                      :remote GIT_BRANCH_REMOTE
                      _ GIT_BRANCH_ALL)
                    GIT_BRANCH_ALL)
           iter (with-pp (fn [pp]
                           (check (c-branch-iterator-new pp repo filter)
                                  "git/branches")))
           results @[]
           ref-pp (ffi/malloc 8)
           type-pp (ffi/malloc 4)]
      (def @done false)
      (while (not done)
        (let [rc (c-branch-next ref-pp type-pp iter)]
          (if (= rc GIT_ITEROVER)
            (assign done true)
            (begin
              (check rc "git/branches")
              (let* [ref-ptr (ffi/read ref-pp :ptr)
                     kind (ffi/read type-pp :i32)
                     name-pp (ffi/malloc 8)
                     _ (c-branch-name name-pp ref-ptr)
                     name (ffi/string (ffi/read name-pp :ptr))
                     target (c-ref-target ref-ptr)
                     oid (if (= target null-ptr) nil (oid->str target))]
                (push results
                      {:name name
                       :oid oid
                       :kind (if (= kind GIT_BRANCH_LOCAL) :local :remote)})
                (c-ref-free ref-ptr)
                (ffi/free name-pp))))))
      (c-branch-iterator-free iter)
      (ffi/free ref-pp)
      (ffi/free type-pp)
      (->list results)))

  (defn branch-create [repo name & opts]
    (let* [target-str (if (> (length opts) 0) (first opts) "HEAD")
           obj (with-pp (fn [pp]
                          (check (c-revparse pp repo target-str)
                                 "git/branch-create")))
           commit (with-pp (fn [pp]
                             (check (c-commit-lookup pp repo (c-object-id obj))
                                    "git/branch-create")))
           branch-ref (with-pp (fn [pp]
                                 (check (c-branch-create pp repo name commit 0)
                                        "git/branch-create")))
           target (c-ref-target branch-ref)
           oid (if (= target null-ptr) nil (oid->str target))]
      (c-ref-free branch-ref)
      (c-commit-free commit)
      (c-object-free obj)
      oid))

  (defn branch-delete [repo name]
    (let [branch (with-pp (fn [pp]
                            (check (c-branch-lookup pp repo name
                                   GIT_BRANCH_LOCAL) "git/branch-delete")))]
      (check (c-branch-delete branch) "git/branch-delete")
      nil))

  ## ── Tags ─────────────────────────────────────────────────────────

  (defn tags [repo]  ## git_strarray: strings (ptr) at 0, count (size_t) at 8
    (let* [sa (ffi/malloc 16)
           _ (check (c-tag-list sa repo) "git/tags")
           count (ffi/read (ptr/add sa 8) :size)
           strings-ptr (ffi/read sa :ptr)
           results @[]]
      (each i in (range count)
        (let [s (ffi/read (ptr/add strings-ptr (* i 8)) :ptr)]
          (push results (ffi/string s))))
      (c-strarray-free sa)
      (ffi/free sa)
      (->list results)))

  (defn tag-create [repo name & opts]
    (let* [target-str (if (> (length opts) 0) (first opts) "HEAD")
           message (if (> (length opts) 1) (nth 1 opts) nil)
           obj (with-pp (fn [pp]
                          (check (c-revparse pp repo target-str)
                                 "git/tag-create")))
           new-oid (ffi/malloc GIT_OID_SIZE)
           rc (if message
                (let [sig (with-pp (fn [pp]
                                     (check (c-sig-now pp "tagger"
                                     "tagger@local") "git/tag-create")))]
                  (let [r (c-tag-create new-oid repo name obj sig message 0)]
                    (c-sig-free sig)
                    r))
                (c-tag-create-lightweight new-oid repo name obj 0))
           _ (check rc "git/tag-create")
           result (oid->str new-oid)]
      (c-object-free obj)
      (ffi/free new-oid)
      result))

  (defn tag-delete [repo name]
    (check (c-tag-delete repo name) "git/tag-delete")
    nil)

  ## ── Remotes ──────────────────────────────────────────────────────

  (defn remotes [repo]
    (let* [sa (ffi/malloc 16)
           _ (check (c-remote-list sa repo) "git/remotes")
           count (ffi/read (ptr/add sa 8) :size)
           strings-ptr (ffi/read sa :ptr)
           results @[]]
      (each i in (range count)
        (push results (ffi/string (ffi/read (ptr/add strings-ptr (* i 8)) :ptr))))
      (c-strarray-free sa)
      (ffi/free sa)
      (->list results)))

  (defn remote-info [repo name]
    (let* [remote (with-pp (fn [pp]
                             (check (c-remote-lookup pp repo name)
                                    "git/remote-info")))
           result {:name name
                   :url (maybe-str (c-remote-url remote))
                   :push-url (maybe-str (c-remote-pushurl remote))}]
      (c-remote-free remote)
      result))

  (defn fetch [repo remote-name]
    (let [remote (with-pp (fn [pp]
                            (check (c-remote-lookup pp repo remote-name)
                                   "git/fetch")))]
      (check (c-remote-fetch remote null-ptr null-ptr null-ptr) "git/fetch")
      (c-remote-free remote)
      nil))

  ## ── Config ───────────────────────────────────────────────────────

  (defn config-get [repo key]
    (let* [config (with-pp (fn [pp]
                             (check (c-repo-config pp repo) "git/config-get")))
           snap (with-pp (fn [pp]
                           (check (c-config-snapshot pp config) "git/config-get")))
           val-pp (ffi/malloc 8)
           rc (c-config-get-string val-pp snap key)
           result (if (zero? rc) (ffi/string (ffi/read val-pp :ptr)) nil)]
      (ffi/free val-pp)
      (c-config-free snap)
      (c-config-free config)
      result))

  (defn config-set [repo key val]
    (let [config (with-pp (fn [pp]
                            (check (c-repo-config pp repo) "git/config-set")))]
      (check (c-config-set-string config key val) "git/config-set")
      (c-config-free config)
      nil))

  {:open open
   :init init
   :clone clone-repo
   :close close
   :path repo-path
   :workdir workdir
   :bare? bare?
   :state state
   :head head
   :resolve resolve
   :commit-info commit-info
   :log log
   :commit commit
   :status status
   :add add
   :remove remove
   :branches branches
   :branch-create branch-create
   :branch-delete branch-delete
   :tags tags
   :tag-create tag-create
   :tag-delete tag-delete
   :remotes remotes
   :remote-info remote-info
   :fetch fetch
   :config-get config-get
   :config-set config-set})
