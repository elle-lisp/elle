#!/usr/bin/env elle
## test-eval.lisp — integration test for the MCP eval tool
##
## Spawns the MCP server, exercises the eval tool contract documented
## in docs/mcp-eval.md.  Counter-factual: these tests must FAIL before
## the eval tool is implemented, and PASS after.
##
## Usage:
##   elle tools/test-eval.lisp                    # uses "elle" in PATH
##   elle tools/test-eval.lisp ./target/debug/elle

(elle/epoch 5)

# ── Configuration ────────────────────────────────────────────────────────

(def test-args (sys/args))

(def elle-bin
  (cond
    ((not (empty? test-args)) (first test-args))
    ((sys/env "ELLE_BIN")     (sys/env "ELLE_BIN"))
    (true                     "elle")))

(def test-store "./target/elle-eval-test-store")

# ── Test harness ─────────────────────────────────────────────────────────

(var pass-count 0)
(var fail-count 0)

(defn test [name ok? msg]
  "Assert a condition; abort the suite on first failure."
  (if ok?
    (begin
      (println "  PASS  " name)
      (assign pass-count (inc pass-count)))
    (begin
      (println "  FAIL  " name " — " msg)
      (assign fail-count (inc fail-count))
      (error {:error :test-failure :message (string name ": " msg)}))))

(defn rm-rf [path]
  (let [[[ok? _] (protect (subprocess/system "rm" ["-rf" path]))]]
    nil))

# ── JSON-RPC I/O helpers ────────────────────────────────────────────────

(var notification-buffer @[])

(defn send [pin msg]
  (port/write pin (json/serialize msg))
  (port/write pin "\n")
  (port/flush pin))

(defn recv-response [pout want-id]
  "Read messages until one with id=want-id arrives."
  (var result nil)
  (while (nil? result)
    (let [[line (port/read-line pout)]]
      (when (nil? line)
        (error {:error :eof :message "server closed stdout"}))
      (let [[msg (json/parse line)]]
        (if (and (not (nil? (get msg "id"))) (= (get msg "id") want-id))
          (assign result msg)
          (when (not (nil? (get msg "method")))
            (push notification-buffer msg))))))
  result)

(defn call-tool [pin pout id name args]
  (send pin {:jsonrpc "2.0" :id id :method "tools/call"
             :params {:name name :arguments args}})
  (recv-response pout id))

(defn tool-text [response]
  "Extract content[0].text from a tools/call response."
  (get (get (get (get response "result") "content") 0) "text"))

(defn tool-error? [response]
  "True if the tool response has isError."
  (get (get response "result") "isError"))

(defn eval-result [response]
  "Parse the eval tool's JSON payload."
  (json/parse (tool-text response)))

# ── Main ────────────────────────────────────────────────────────────────

(println "── MCP eval tool integration test ──")
(println "  elle-bin:  " elle-bin)
(println "  store:     " test-store)

(rm-rf test-store)

(def proc
  (subprocess/exec elle-bin ["tools/mcp-server.lisp" "--" test-store]))
(def pin  (get proc :stdin))
(def pout (get proc :stdout))

# Handles saved across tests
(var handle-42  nil)
(var handle-43  nil)
(var handle-85  nil)
(var handle-err nil)

(defer (begin (subprocess/kill proc) (rm-rf test-store))

  # ── Initialize ────────────────────────────────────────────────────────
  (let [[[ok? r] (protect
      (ev/timeout 10 (fn []
        (send pin {:jsonrpc "2.0" :id 1 :method "initialize"
                   :params {:protocolVersion "2025-03-26"
                            :capabilities {}
                            :clientInfo {:name "test-eval" :version "0.1"}}})
        (recv-response pout 1))))]]
    (test "initialize" ok? "server did not respond within 10 seconds"))

  (send pin {:jsonrpc "2.0" :method "notifications/initialized"})

  # ── Verify eval tool is listed ────────────────────────────────────────
  (send pin {:jsonrpc "2.0" :id 2 :method "tools/list" :params {}})
  (let* [[r (recv-response pout 2)]
         [tools (get (get r "result") "tools")]
         [names (map (fn [t] (get t "name")) tools)]
         [has-eval (not (nil? (find (fn [n] (= n "eval")) names)))]]
    (test "tools/list: eval is listed" has-eval
      (string "tool list does not contain 'eval': " (string/join (->list names) ", "))))

  # ── 1. Nullary eval — compute a value from nothing ────────────────────
  (let* [[r (call-tool pin pout 10 "eval"
              {:lambda "(fn [] 42)"})]
         [payload (eval-result r)]]
    (test "eval nullary: ok" (get payload "ok") (tool-text r))
    (test "eval nullary: kind is :integer"
      (= (get payload "kind") ":integer")
      (string "got kind " (get payload "kind")))
    (test "eval nullary: has handle"
      (not (nil? (get payload "handle")))
      "missing handle")
    (assign handle-42 (get payload "handle")))

  # ── 2. Unary eval — compose against a prior handle ────────────────────
  (let* [[r (call-tool pin pout 11 "eval"
              {:lambda "(fn [n] (+ n 1))"
               :inputs [handle-42]})]
         [payload (eval-result r)]]
    (test "eval unary: ok" (get payload "ok") (tool-text r))
    (test "eval unary: kind is :integer"
      (= (get payload "kind") ":integer")
      (string "got kind " (get payload "kind")))
    (assign handle-43 (get payload "handle")))

  # ── 3. Verify value — project it via identity + println ───────────────
  (let* [[r (call-tool pin pout 12 "eval"
              {:lambda "(fn [v] (println v) v)"
               :inputs [handle-43]})]
         [payload (eval-result r)]]
    (test "eval project: stdout captures println"
      (string/contains? (get payload "stdout") "43")
      (string "stdout was: " (get payload "stdout")))
    (test "eval project: kind is :integer"
      (= (get payload "kind") ":integer")
      (string "got kind " (get payload "kind"))))

  # ── 4. Multi-arity — combine two handles ──────────────────────────────
  (let* [[r (call-tool pin pout 13 "eval"
              {:lambda "(fn [a b] (+ a b))"
               :inputs [handle-42 handle-43]})]
         [payload (eval-result r)]]
    (test "eval multi-arity: ok" (get payload "ok") (tool-text r))
    (assign handle-85 (get payload "handle")))

  # ── 5. String value — kind and shape ──────────────────────────────────
  (let* [[r (call-tool pin pout 14 "eval"
              {:lambda "(fn [] \"hello world\")"})]
         [payload (eval-result r)]]
    (test "eval string: ok" (get payload "ok") (tool-text r))
    (test "eval string: kind is :string"
      (= (get payload "kind") ":string")
      (string "got kind " (get payload "kind"))))

  # ── 6. Collection value — shape has count ─────────────────────────────
  (let* [[r (call-tool pin pout 15 "eval"
              {:lambda "(fn [] [1 2 3 4 5])"})]
         [payload (eval-result r)]]
    (test "eval collection: ok" (get payload "ok") (tool-text r))
    (test "eval collection: kind is :array"
      (= (get payload "kind") ":array")
      (string "got kind " (get payload "kind")))
    (test "eval collection: shape has count"
      (= (get (get payload "shape") "count") 5)
      (string "shape: " (json/serialize (get payload "shape")))))

  # ── 7. Lambda that throws — ok:false with error handle ────────────────
  (let* [[r (call-tool pin pout 16 "eval"
              {:lambda "(fn [] (error {:error :boom :message \"kaboom\"}))"})]
         [payload (eval-result r)]]
    (test "eval throw: ok is false" (not (get payload "ok")) (tool-text r))
    (test "eval throw: kind is :error"
      (= (get payload "kind") ":error")
      (string "got kind " (get payload "kind")))
    (test "eval throw: has handle"
      (not (nil? (get payload "handle")))
      "missing error handle")
    (assign handle-err (get payload "handle")))

  # ── 8. Probe error handle — get the reason ────────────────────────────
  (let* [[r (call-tool pin pout 17 "eval"
              {:lambda "(fn [e] (get e :message))"
               :inputs [handle-err]})]
         [payload (eval-result r)]]
    (test "eval probe error: ok" (get payload "ok") (tool-text r))
    (test "eval probe error: kind is :string"
      (= (get payload "kind") ":string")
      (string "got kind " (get payload "kind"))))

  # ── 9. Unknown handle — protocol error ────────────────────────────────
  (let* [[r (call-tool pin pout 18 "eval"
              {:lambda "(fn [x] x)"
               :inputs ["nonexistent-handle-12345"]})]
         [is-err (tool-error? r)]]
    (test "eval unknown handle: isError" is-err
      (string "expected isError, got: " (tool-text r))))

  # ── 10. Arity mismatch — protocol error ──────────────────────────────
  (let* [[r (call-tool pin pout 19 "eval"
              {:lambda "(fn [a b] (+ a b))"
               :inputs [handle-42]})]
         [is-err (tool-error? r)]]
    (test "eval arity mismatch: isError" is-err
      (string "expected isError, got: " (tool-text r))))

  # ── 11. Non-callable lambda — protocol error ─────────────────────────
  (let* [[r (call-tool pin pout 20 "eval"
              {:lambda "42"})]
         [is-err (tool-error? r)]]
    (test "eval non-callable: isError" is-err
      (string "expected isError, got: " (tool-text r))))

  # ── 12. Stderr capture ───────────────────────────────────────────────
  (let* [[r (call-tool pin pout 21 "eval"
              {:lambda "(fn [] (eprintln \"debug info\") :ok)"})]
         [payload (eval-result r)]]
    (test "eval stderr: ok" (get payload "ok") (tool-text r))
    (test "eval stderr: captured"
      (string/contains? (get payload "stderr") "debug info")
      (string "stderr was: " (get payload "stderr"))))

  # ── 13. Timeout ──────────────────────────────────────────────────────
  (let* [[r (call-tool pin pout 22 "eval"
              {:lambda "(fn [] (ev/sleep 100))"
               :timeout_ms 200})]
         [payload (eval-result r)]]
    (test "eval timeout: ok is false" (not (get payload "ok")) (tool-text r))
    (test "eval timeout: kind is :error"
      (= (get payload "kind") ":error")
      (string "got kind " (get payload "kind"))))

  # ── 14. Duration is present ──────────────────────────────────────────
  (let* [[r (call-tool pin pout 23 "eval"
              {:lambda "(fn [] (+ 1 2 3))"})]
         [payload (eval-result r)]]
    (test "eval duration: present and positive"
      (> (get payload "duration_ns") 0)
      (string "duration_ns: " (get payload "duration_ns"))))

  # ── 15. Inputs default to empty ──────────────────────────────────────
  (let* [[r (call-tool pin pout 24 "eval"
              {:lambda "(fn [] :no-inputs)"})]
         [payload (eval-result r)]]
    (test "eval no-inputs: ok" (get payload "ok") (tool-text r))
    (test "eval no-inputs: kind is :keyword"
      (= (get payload "kind") ":keyword")
      (string "got kind " (get payload "kind"))))

  (println "")
  (println (string pass-count " passed, " fail-count " failed"))
  (when (> fail-count 0)
    (error {:error :test-failure :message (string fail-count " tests failed")}))
  (println "all eval tests passed."))
