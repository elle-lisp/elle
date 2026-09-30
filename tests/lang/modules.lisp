(elle/epoch 14)
# audited: 2026-09-30
# import-file loads a module file as a function of its parameters; each import runs the file again, and a call answers its exports.
# docs/modules.md

# ============================================================================
# 1. Basic parametric import with qualified symbol access
# ============================================================================

(let [fmt ((import-file "tests/modules/formatter.lisp") :prefix "[" :suffix "]"
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

(let [brackets ((import-file "tests/modules/formatter.lisp") :prefix "("
      :suffix ")")
      angles ((import-file "tests/modules/formatter.lisp") :prefix "<"
      :suffix ">")]
  (assert (= (brackets:wrap "x") "(x)") "two instances: brackets wrap")
  (assert (= (angles:wrap "x") "<x>") "two instances: angles wrap")
  (assert (= (brackets:join ["a" "b"]) "a, b")
          "two instances: brackets default separator")
  (assert (= (angles:join ["a" "b"]) "a, b")
          "two instances: angles default separator"))

# ============================================================================
# 3. Default parameters (no keyword args)
# ============================================================================

(let [fmt ((import-file "tests/modules/formatter.lisp"))]
  (assert (= (fmt:wrap "hello") "hello")
          "defaults: wrap with empty prefix/suffix")
  (assert (= (fmt:join ["a" "b" "c"]) "a, b, c")
          "defaults: join with default separator"))

# ============================================================================
# 4. Selective destructuring import
# ============================================================================

(let [{:wrap wrap :upper upper} ((import-file "tests/modules/formatter.lisp") :prefix "<<"
      :suffix ">>")]
  (assert (= (wrap "hi") "<<hi>>") "destructured: wrap")
  (assert (= (upper "hi") "HI") "destructured: upper"))

# ============================================================================
# 5. Module as first-class value
# ============================================================================

(defn apply-wrap [mod s]
  "Call wrap from a module struct."
  (mod:wrap s))

(let [fmt ((import-file "tests/modules/formatter.lisp") :prefix "{" :suffix "}")]
  (assert (= (apply-wrap fmt "val") "{val}")
          "first-class: pass module to function"))

# ============================================================================
# 6. Letrec isolation — defn in imported file does not leak into caller scope
# ============================================================================

# Files are compiled as a single letrec. Top-level defn forms are local to
# the file. The only way to get definitions out is via the return value.
# Importing without binding the result gives you the side effects only.

(let [[ok? _] (protect (eval '(do
                                (import-file "tests/modules/counter.lisp")
                                (inc))))]
  (assert (not ok?) "defn in imported file is not visible in caller scope"))

# ============================================================================
# 7. Existing module fixtures
# ============================================================================

# test.lisp — simple value exports
(let [test-mod ((import-file "tests/modules/test.lisp"))]
  (assert (= test-mod:test-var 42) "test.lisp: test-var")
  (assert (= test-mod:test-string "hello") "test.lisp: test-string")
  (assert (= test-mod:test-list (list 1 2 3)) "test.lisp: test-list"))

# a path that starts with ./ names the same file
(let [dotted ((import-file "./tests/modules/test.lisp"))]
  (assert (= dotted:test-var 42) "a ./ path imports the same module"))

# counter.lisp — stateful module (import-file re-executes, giving independent state)
(let [c1 ((import-file "tests/modules/counter.lisp"))
      c2 ((import-file "tests/modules/counter.lisp"))]
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
# "import: unexpected signal 0x2000".
(let [m ((import-file "tests/modules/protect-toplevel.lisp"))]
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
