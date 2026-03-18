# file/stat and file/lstat — error cases and symlink behavior

(def {:assert-eq assert-eq
      :assert-true assert-true
      :assert-false assert-false
      :assert-err-kind assert-err-kind}
  ((import-file "tests/elle/assert.lisp")))

# ── file/stat error cases ────────────────────────────────────────────────────

# not-found → io-error
(assert-err-kind
  (fn [] (file/stat "/tmp/elle-nonexistent-path-stat-test"))
  :io-error
  "file/stat not-found gives io-error")

# non-string argument → type-error
(assert-err-kind
  (fn [] (file/stat 42))
  :type-error
  "file/stat non-string gives type-error")

# wrong arity → arity-error (use apply to bypass compile-time arity check)
(assert-err-kind
  (fn [] (apply file/stat ["a" "b"]))
  :arity-error
  "file/stat wrong arity gives arity-error")

# ── file/lstat error cases ───────────────────────────────────────────────────

# not-found → io-error
(assert-err-kind
  (fn [] (file/lstat "/tmp/elle-nonexistent-path-lstat-test"))
  :io-error
  "file/lstat not-found gives io-error")

# non-string argument → type-error
(assert-err-kind
  (fn [] (file/lstat 42))
  :type-error
  "file/lstat non-string gives type-error")

# wrong arity → arity-error (use apply to bypass compile-time arity check)
(assert-err-kind
  (fn [] (apply file/lstat ["a" "b"]))
  :arity-error
  "file/lstat wrong arity gives arity-error")

# ── symlink behavior ─────────────────────────────────────────────────────────

(def sym-dir "/tmp/elle-test-stat-symlink")
(file/mkdir-all sym-dir)
(def target-path (path/join sym-dir "target.txt"))
(def link-path   (path/join sym-dir "link.txt"))
(file/write target-path "hello")

# create symlink: ln -sf <target> <link>
(ev/spawn (fn []
  (subprocess/system "ln" ["-sf" target-path link-path])))

# file/stat follows the symlink — reports the target's metadata
(def stat-info (file/stat link-path))
(assert-true  (get stat-info :is-file)    "file/stat through symlink sees file")
(assert-false (get stat-info :is-symlink) "file/stat through symlink: is-symlink false")
(assert-eq    (get stat-info :file-type)  "file"  "file/stat through symlink: file-type is file")
(assert-eq    (get stat-info :size)       5       "file/stat through symlink sees target size")

# file/lstat does not follow — reports the symlink's own metadata
(def lstat-info (file/lstat link-path))
(assert-true  (get lstat-info :is-symlink) "file/lstat on symlink: is-symlink true")
(assert-false (get lstat-info :is-file)    "file/lstat on symlink: is-file false")
(assert-false (get lstat-info :is-dir)     "file/lstat on symlink: is-dir false")
(assert-eq    (get lstat-info :file-type)  "symlink" "file/lstat on symlink: file-type is symlink")

# cleanup
(file/delete link-path)
(file/delete target-path)
(file/delete-dir sym-dir)
