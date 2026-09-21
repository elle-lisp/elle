(elle/epoch 12)
## audited: 2026-09-21
## lib/git/write.lisp — every mutation of a repository: staging,
## commits, branches, tags, config, and fetch.
##
## Loaded via: (def wr ((import "std/git/write") :core core))

(fn [&named core]
  (def cfn core:cfn)
  (def check core:check)
  (def with-pp core:with-pp)
  (def oid->str core:oid->str)
  (def maybe-str core:maybe-str)
  (def null-ptr core:null-ptr)
  (def GIT_OID_SIZE core:oid-size)

  ## ── C bindings ───────────────────────────────────────────────────

  (def c-repo-head (cfn "git_repository_head" :int @[:ptr :ptr]))
  (def c-repo-index (cfn "git_repository_index" :int @[:ptr :ptr]))
  (def c-repo-config (cfn "git_repository_config" :int @[:ptr :ptr]))

  (def c-ref-target (cfn "git_reference_target" :ptr @[:ptr]))
  (def c-ref-free (cfn "git_reference_free" :void @[:ptr]))

  (def c-revparse (cfn "git_revparse_single" :int @[:ptr :ptr :string]))
  (def c-object-id (cfn "git_object_id" :ptr @[:ptr]))
  (def c-object-free (cfn "git_object_free" :void @[:ptr]))

  (def c-commit-lookup (cfn "git_commit_lookup" :int @[:ptr :ptr :ptr]))
  (def c-commit-free (cfn "git_commit_free" :void @[:ptr]))

  ## git_signature: name at offset 0 (ptr), email at offset 8 (ptr), when at offset 16
  ## when is git_time: time (i64) at +0, offset (int) at +8
  (def c-sig-now (cfn "git_signature_now" :int @[:ptr :string :string]))
  (def c-sig-free (cfn "git_signature_free" :void @[:ptr]))

  (def c-index-add-bypath (cfn "git_index_add_bypath" :int @[:ptr :string]))
  (def c-index-remove-bypath
    (cfn "git_index_remove_bypath" :int @[:ptr :string]))
  (def c-index-write (cfn "git_index_write" :int @[:ptr]))
  (def c-index-write-tree (cfn "git_index_write_tree" :int @[:ptr :ptr]))
  (def c-index-free (cfn "git_index_free" :void @[:ptr]))

  (def c-tree-lookup (cfn "git_tree_lookup" :int @[:ptr :ptr :ptr]))
  (def c-tree-free (cfn "git_tree_free" :void @[:ptr]))

  (def c-commit-create
    (cfn "git_commit_create"
         :int @[:ptr :ptr :string :ptr :ptr :ptr :string :ptr :size :ptr]))

  (def c-branch-create
    (cfn "git_branch_create" :int @[:ptr :ptr :string :ptr :int]))
  (def c-branch-delete (cfn "git_branch_delete" :int @[:ptr]))
  (def c-branch-lookup (cfn "git_branch_lookup" :int @[:ptr :ptr :string :int]))

  (def c-tag-create-lightweight
    (cfn "git_tag_create_lightweight" :int @[:ptr :ptr :string :ptr :int]))
  (def c-tag-create
    (cfn "git_tag_create" :int @[:ptr :ptr :string :ptr :ptr :string :int]))
  (def c-tag-delete (cfn "git_tag_delete" :int @[:ptr :string]))

  (def c-remote-lookup (cfn "git_remote_lookup" :int @[:ptr :ptr :string]))
  (def c-remote-fetch (cfn "git_remote_fetch" :int @[:ptr :ptr :ptr :ptr]))
  (def c-remote-free (cfn "git_remote_free" :void @[:ptr]))

  (def c-config-get-string
    (cfn "git_config_get_string" :int @[:ptr :ptr :string]))
  (def c-config-set-string
    (cfn "git_config_set_string" :int @[:ptr :string :string]))
  (def c-config-snapshot (cfn "git_config_snapshot" :int @[:ptr :ptr]))
  (def c-config-free (cfn "git_config_free" :void @[:ptr]))

  (def GIT_BRANCH_LOCAL 1)

  ## ── Staging and commits ──────────────────────────────────────────

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

  ## ── Branches, tags, config, remotes ──────────────────────────────

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

  (defn config-set [repo key val]
    (let [config (with-pp (fn [pp]
                            (check (c-repo-config pp repo) "git/config-set")))]
      (check (c-config-set-string config key val) "git/config-set")
      (c-config-free config)
      nil))

  (defn fetch [repo remote-name]
    (let [remote (with-pp (fn [pp]
                            (check (c-remote-lookup pp repo remote-name)
                                   "git/fetch")))]
      (check (c-remote-fetch remote null-ptr null-ptr null-ptr) "git/fetch")
      (c-remote-free remote)
      nil))

  {:add add
   :remove remove
   :commit commit
   :branch-create branch-create
   :branch-delete branch-delete
   :tag-create tag-create
   :tag-delete tag-delete
   :config-set config-set
   :fetch fetch})
