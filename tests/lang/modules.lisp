(elle/epoch 14)
# audited: 2026-10-06
# import-file loads a module file as a function of its parameters; each import runs the file again, and a call answers its exports.
# docs/modules.md

# ============================================================================
# 1. Basic parametric import with qualified symbol access
# ============================================================================

(let [fmt ((import-file "../modules/formatter.lisp") :prefix "[" :suffix "]"
      :separator " | ")]
  (assert (= (fmt:wrap "hello") "[hello]")
          "qualified access: wrap with prefix/suffix")
  (assert (= (fmt:join [1 2 3]) "1 | 2 | 3")
          "qualified access: join with separator")
  (assert (= (fmt:upper "hello") "HELLO")
          "qualified access: upper (unconfigured)")
  (assert (= (fmt:identity 42) 42) "qualified access: identity"))

# ============================================================================
# 2. Two instances with different configurations
# ============================================================================

(let [brackets ((import-file "../modules/formatter.lisp") :prefix "("
      :suffix ")")
      angles ((import-file "../modules/formatter.lisp") :prefix "<" :suffix ">")]
  (assert (= (brackets:wrap "x") "(x)") "two instances: brackets wrap")
  (assert (= (angles:wrap "x") "<x>") "two instances: angles wrap")
  (assert (= (brackets:join ["a" "b"]) "a, b")
          "two instances: brackets default separator")
  (assert (= (angles:join ["a" "b"]) "a, b")
          "two instances: angles default separator"))

# ============================================================================
# 3. Default parameters (no keyword args)
# ============================================================================

(let [fmt ((import-file "../modules/formatter.lisp"))]
  (assert (= (fmt:wrap "hello") "hello")
          "defaults: wrap with empty prefix/suffix")
  (assert (= (fmt:join ["a" "b" "c"]) "a, b, c")
          "defaults: join with default separator"))

# ============================================================================
# 4. Selective destructuring import
# ============================================================================

(let [{:wrap wrap :upper upper} ((import-file "../modules/formatter.lisp") :prefix "<<"
      :suffix ">>")]
  (assert (= (wrap "hi") "<<hi>>") "destructured: wrap")
  (assert (= (upper "hi") "HI") "destructured: upper"))

# ============================================================================
# 5. Module as first-class value
# ============================================================================

(defn apply-wrap [mod s]
  "Call wrap from a module struct."
  (mod:wrap s))

(let [fmt ((import-file "../modules/formatter.lisp") :prefix "{" :suffix "}")]
  (assert (= (apply-wrap fmt "val") "{val}")
          "first-class: pass module to function"))

# ============================================================================
# 6. Letrec isolation — defn in imported file does not leak into caller scope
# ============================================================================

# Files are compiled as a single letrec. Top-level defn forms are local to
# the file. The only way to get definitions out is via the return value.
# Importing without binding the result gives you the side effects only.

# A datum that eval compiles has no file, so its path resolves against the
# working directory, the repository root under the runner. The first form
# proves the load succeeds; without it the second would pass on a missing file.
# The stdlib has an `inc` of one argument, so a call of none fails unless the
# module's `inc` of none leaked into this scope.
(let [[ok? _] (protect (eval '(import-file "tests/modules/counter.lisp")))]
  (assert ok? "an eval'd import-file resolves against the working directory"))
(let [[ok? err] (protect (eval '(do
                                  (import-file "tests/modules/counter.lisp")
                                  (inc))))]
  (assert (not ok?) "defn in imported file is not visible in caller scope")
  (assert (string/contains? (get err :message) "arity-error")
          "the call reached the stdlib inc, not the module's"))

# ============================================================================
# 7. Existing module fixtures
# ============================================================================

# test.lisp — simple value exports
(let [test-mod ((import-file "../modules/test.lisp"))]
  (assert (= test-mod:test-var 42) "test.lisp: test-var")
  (assert (= test-mod:test-string "hello") "test.lisp: test-string")
  (assert (= test-mod:test-list (list 1 2 3)) "test.lisp: test-list"))

# a path that starts with ./ names the same file
(let [dotted ((import-file "./../modules/test.lisp"))]
  (assert (= dotted:test-var 42) "a ./ path imports the same module"))

# counter.lisp — stateful module (import-file re-executes, giving independent state)
(let [c1 ((import-file "../modules/counter.lisp"))
      c2 ((import-file "../modules/counter.lisp"))]
  (c1:inc)
  (c1:inc)
  (c1:inc)
  (assert (= (c1:count) 3) "counter: c1 incremented three times")
  (assert (= (c2:count) 0) "counter: c2 independent, still zero")
  (c2:inc)
  (assert (= (c2:count) 1) "counter: c2 incremented once"))

# ============================================================================
# 8. Top-level `protect` in an imported module — SIG_SWITCH must not leak
# ============================================================================

# `protect` expands to fiber/new + fiber/resume. An imported module's forms run
# nested inside the caller's fiber, so that resume returns the VM-internal
# SIG_SWITCH trampoline signal. The import executor must drain it (as the root
# dispatch and `eval` do); if it leaks, the import dies with
# "import/load-file: unexpected signal 0x2000".
(let [m ((import-file "../modules/protect-toplevel.lisp"))]
  (assert (= (m:ok) true)
          "top-level protect module imports without leaking SIG_SWITCH"))

# ============================================================================
# 9. import-file refuses what names no module
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
# 10. import-file is raw: no prefix, no search, no suffix
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
(let [[built? _] (protect (import "plugin/myplugin"))]
  (when built?
    (let [[ok? _] (protect (import-file "plugin/myplugin"))]
      (assert (not ok?) "import-file applies no plugin/ prefix"))))

# ============================================================================
# 11. A relative path names a file beside the writer, wherever it runs
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

# ============================================================================
# 12. Neither import-file nor import is a value
# ============================================================================

(assert (macro? import) "import is a macro")
(let [[ok? _] (protect (eval 'import-file))]
  (assert (not ok?) "import-file is a special form, not a value"))
(let [[ok? _] (protect (eval 'import))]
  (assert (not ok?) "import is a macro, not a value"))
(let [[ok? _] (protect (eval 'module/import))]
  (assert (not ok?) "module/import is gone"))
