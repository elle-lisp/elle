(elle/epoch 9)
## tests/http2/all.lisp — run all HTTP/2 tests in sequence

(defn run-file [path]
  (println "=== " path " ===")
  (let [proc (subprocess/exec "elle"
                              ["--home=." path]
                              {:stdout :pipe :stderr :pipe})]
    (let [out (string (port/read-all (get proc :stdout)))
          err (string (port/read-all (get proc :stderr)))
          code (subprocess/wait proc)]
      (when (> (length out) 0) (println out))
      (when (not (= code 0))
        (when (> (length err) 0) (println err))
        (error {:error :test-failure
                :path path
                :message (concat path " failed with exit code " (string code))})))))

(run-file "tests/http2/modules.lisp")
(run-file "tests/http2/scheduler.lisp")
(run-file "tests/http2/flow.lisp")
(run-file "tests/http2/server.lisp")

(println "all HTTP/2 tests passed")
