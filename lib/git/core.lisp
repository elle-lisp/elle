(elle/epoch 12)
## audited: 2026-09-21
## lib/git/core.lisp — the libgit2 mapping and the helpers every git
## submodule builds its bindings from.
##
## Loaded via: (def core ((import "std/git/core")))

(fn []
  (def lib (ffi/native "libgit2.so"))
  (def null-ptr (ptr/from-int 0))

  (defn cfn [name ret args]
    (let [p (ffi/lookup lib name)
          s (ffi/signature ret args)]
      (fn [& a] (apply ffi/call p s a))))

  ## Initialize libgit2, and declare its teardown as this library's unload
  ## destructor. The mapping is process-global and never unloaded, so this is
  ## optional graceful cleanup an explicit (ffi/run-teardowns) runs — never an
  ## obligation to avoid a crash (see `shutdown` in lib/git.lisp).
  ((cfn "git_libgit2_init" :int @[]))
  (ffi/on-unload lib "git_libgit2_shutdown")

  (def c-error-last (cfn "git_error_last" :ptr @[]))
  (def c-oid-tostr (cfn "git_oid_tostr_s" :ptr @[:ptr]))

  ## OID size: 20 bytes
  (def GIT_OID_SIZE 20)

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

  {:cfn cfn
   :check check
   :with-pp with-pp
   :oid->str oid->str
   :maybe-str maybe-str
   :sig->struct sig->struct
   :null-ptr null-ptr
   :oid-size GIT_OID_SIZE})
