(elle/epoch 15)
# audited: 2026-10-23
# import-file takes its path as written, beside its writer, and refuses one that names no module; neither loader is a value.
# docs/modules.md

# ============================================================================
# import-file is raw: no prefix, no search, no suffix
# ============================================================================

# Each of these names a module `import` finds. `import-file` takes the path as
# written, so none names a file. The counter-factual is an import-file that is
# import under another name: every one of them loads.
(let [[ok? err] (protect (import-file "std/base64"))]
  (assert (and (not ok?) (= (get err :error) :io-error))
          "import-file applies no std/ prefix"))
(assert (path/file? (path/join (path/parent (get (meta/location) :file))
                               "../../lib/base64.lisp"))
        "the file a .lisp suffix would name exists")
(let [[ok? _] (protect (import-file "../../lib/base64"))]
  (assert (not ok?) "import-file tries no .lisp suffix"))
# Only a build that made the plugin can show the prefix is ignored.
(let [[built? _] (protect (import "plugin/hash"))]
  (when built?
    (let [[ok? _] (protect (import-file "plugin/hash"))]
      (assert (not ok?) "import-file applies no plugin/ prefix"))))

# ============================================================================
# import-file refuses what names no module
# ============================================================================

(let [[ok? err] (protect (import-file "./lib/nonexistent.lisp"))]
  (assert (and (not ok?) (= (get err :error) :io-error))
          "a relative path that names no file fails with an :io-error"))
(let [[ok? err] (protect (import-file "/absolute/nonexistent.lisp"))]
  (assert (and (not ok?) (= (get err :error) :io-error))
          "an absolute path that names no file fails with an :io-error"))
(let [[ok? err] (protect (import-file 42))]
  (assert (and (not ok?) (= (get err :error) :type-error))
          "import-file refuses an int"))
(let [[ok? err] (protect (import-file nil))]
  (assert (and (not ok?) (= (get err :error) :type-error))
          "import-file refuses nil"))
(let [[ok? _] (protect ((fn () (eval '(import-file)))))]
  (assert (not ok?) "import-file with no arguments fails"))
(let [[ok? _] (protect ((fn () (eval '(import-file "a" "b")))))]
  (assert (not ok?) "import-file with two arguments fails"))

# ============================================================================
# A relative path names a file beside the writer, wherever it runs
# ============================================================================

# The runner starts in the repository root, where ../modules names nothing, so
# a working-directory rule fails both of these.
(let [rel "../modules/test.lisp"
      m ((import-file rel))]
  (assert (= m:test-var 42)
          "a computed relative path joins the writer's directory"))

# importer.lisp loads ./test from inside a function. The call comes from this
# file, and the path still resolves beside importer.lisp, which wrote it.
(let [importer ((import-file "../modules/importer.lisp"))
      m (importer:load)]
  (assert (= m:test-var 42)
          "a path resolves against the file that wrote it, not the caller's"))

# A datum that eval compiles has no file, so its path resolves against the
# working directory, the repository root under the runner.
(let [[ok? _] (protect (eval '(import-file "tests/modules/counter.lisp")))]
  (assert ok? "an eval'd import-file resolves against the working directory"))

# ============================================================================
# Neither import-file nor import is a value
# ============================================================================

(assert (macro? import) "import is a macro")
(let [[ok? _] (protect (eval 'import-file))]
  (assert (not ok?) "import-file is a special form, not a value"))
(let [[ok? _] (protect (eval 'import))]
  (assert (not ok?) "import is a macro, not a value"))
(let [[ok? _] (protect (eval 'module/import))]
  (assert (not ok?) "module/import is gone"))
