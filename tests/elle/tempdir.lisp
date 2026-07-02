(elle/epoch 10)
## tests/elle/tempdir.lisp — file/mktempdir, file/delete-dir-all, with-temp-dir
##
## Temp paths come from the platform temp root (std::env::temp_dir in the
## runtime: TMPDIR on Unix, the per-user folder on macOS, %TEMP% on
## Windows) — never a hardcoded /tmp. Uniqueness is made in the runtime
## (pid + counter, retried on collision), so concurrent processes cannot
## race each other to the same name the way fixed scratch filenames do.

(println "tests/elle/tempdir.lisp:")

## ── file/mktempdir: fresh, absolute, writable, distinct ────────────

(let [d1 (file/mktempdir)
      d2 (file/mktempdir)]
  (assert (string? d1) "mktempdir returns a string")
  (assert (path/absolute? d1) "mktempdir path is absolute")
  (assert (path/dir? d1) "mktempdir directory exists")
  (assert (not (= d1 d2)) "two calls return distinct directories")
  (let [f (path/join d1 "scratch.txt")]
    (file/write f "hello")
    (assert (= (file/read f) "hello") "temp dir is writable"))
  (println "  file/mktempdir: ok")

  ## ── file/delete-dir-all: removes non-empty trees ─────────────────

  (file/mkdir (path/join d1 "sub"))
  (file/write (path/join d1 "sub" "nested.txt") "x")
  (assert (= (file/delete-dir-all d1) true) "delete-dir-all returns true")
  (assert (not (path/exists? d1)) "tree is gone")
  (file/delete-dir-all d2)
  (assert (not (path/exists? d2)) "second tree is gone")
  (println "  file/delete-dir-all: ok"))

## ── with-temp-dir: binds, returns body value, cleans up ────────────

(def @seen nil)
(let [result (with-temp-dir dir (assign seen dir)
                            (assert (path/dir? dir) "dir exists inside body")
                            (file/write (path/join dir "f") "data") :body-value)]
  (assert (= result :body-value) "with-temp-dir returns body value")
  (assert (not (path/exists? seen)) "dir removed after body"))
(println "  with-temp-dir: ok")

## ── with-temp-dir: cleanup runs when the body errors ───────────────

(assign seen nil)
(let [[ok? _] (protect ((fn []
                          (with-temp-dir dir (assign seen dir)
                          (file/write (path/join dir "f") "data") (error :boom)))))]
  (assert (not ok?) "error propagates out of with-temp-dir")
  (assert (string? seen) "body ran before the error")
  (assert (not (path/exists? seen)) "dir removed after error"))
(println "  with-temp-dir cleanup on error: ok")

(println "tests/elle/tempdir.lisp: all tests passed")
