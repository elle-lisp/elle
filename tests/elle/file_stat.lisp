# file/stat and file/lstat — error cases and symlink behavior


# ── file/stat error cases ────────────────────────────────────────────────────

# not-found → io-error
(let (([ok? err] (protect ((fn [] (file/stat "/tmp/elle-nonexistent-path-stat-test")))))) (assert (not ok?) "file/stat not-found gives io-error") (assert (= (get err :error) :io-error) "file/stat not-found gives io-error"))

# non-string argument → type-error
(let (([ok? err] (protect ((fn [] (file/stat 42)))))) (assert (not ok?) "file/stat non-string gives type-error") (assert (= (get err :error) :type-error) "file/stat non-string gives type-error"))

# wrong arity → arity-error (use apply to bypass compile-time arity check)
(let (([ok? err] (protect ((fn [] (apply file/stat ["a" "b"])))))) (assert (not ok?) "file/stat wrong arity gives arity-error") (assert (= (get err :error) :arity-error) "file/stat wrong arity gives arity-error"))

# ── file/lstat error cases ───────────────────────────────────────────────────

# not-found → io-error
(let (([ok? err] (protect ((fn [] (file/lstat "/tmp/elle-nonexistent-path-lstat-test")))))) (assert (not ok?) "file/lstat not-found gives io-error") (assert (= (get err :error) :io-error) "file/lstat not-found gives io-error"))

# non-string argument → type-error
(let (([ok? err] (protect ((fn [] (file/lstat 42)))))) (assert (not ok?) "file/lstat non-string gives type-error") (assert (= (get err :error) :type-error) "file/lstat non-string gives type-error"))

# wrong arity → arity-error (use apply to bypass compile-time arity check)
(let (([ok? err] (protect ((fn [] (apply file/lstat ["a" "b"])))))) (assert (not ok?) "file/lstat wrong arity gives arity-error") (assert (= (get err :error) :arity-error) "file/lstat wrong arity gives arity-error"))

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
(assert (get stat-info :is-file) "file/stat through symlink sees file")
(assert (not (get stat-info :is-symlink)) "file/stat through symlink: is-symlink false")
(assert (= (get stat-info :file-type) "file") "file/stat through symlink: file-type is file")
(assert (= (get stat-info :size) 5) "file/stat through symlink sees target size")

# file/lstat does not follow — reports the symlink's own metadata
(def lstat-info (file/lstat link-path))
(assert (get lstat-info :is-symlink) "file/lstat on symlink: is-symlink true")
(assert (not (get lstat-info :is-file)) "file/lstat on symlink: is-file false")
(assert (not (get lstat-info :is-dir)) "file/lstat on symlink: is-dir false")
(assert (= (get lstat-info :file-type) "symlink") "file/lstat on symlink: file-type is symlink")

# cleanup
(file/delete link-path)
(file/delete target-path)
(file/delete-dir sym-dir)
