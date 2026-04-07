# Agent library tests

(def agent ((import-file "lib/agent.lisp")))

# Resolve the elle binary — ELLE_BIN from Makefile, fallback to "elle"
(def elle-bin (or (sys/env "ELLE_BIN") "elle"))


# ── build-args: flag adjacency helper ──────────────────────────────────────

# Check that flag and value appear adjacent in args array
(defn has-flag-pair [args flag value]
  "True if flag appears immediately before value in args."
  (var i 0)
  (var found false)
  (while (< i (- (length args) 1))
    (when (and (= (get args i) flag)
               (= (get args (+ i 1)) value))
      (assign found true))
    (assign i (+ i 1)))
  found)


# ── build-args ──────────────────────────────────────────────────────────────

# Basic claude args
(let [[args (agent:build-args {:backend :claude} "hello" nil)]]
  (assert (= (first args) "claude") "build-args: claude binary")
  (assert (has-flag-pair args "--output-format" "stream-json") "build-args: output format pair")
  (assert (= (last args) "hello") "build-args: prompt is last")
  (assert (has-flag-pair args "-p" "hello") "build-args: -p prompt pair"))

# Claude with model — flag and value adjacent
(let [[args (agent:build-args {:backend :claude :model "sonnet"} "hi" nil)]]
  (assert (has-flag-pair args "--model" "sonnet") "build-args: --model sonnet pair"))

# Claude with session resume
(let [[args (agent:build-args {:backend :claude} "hi" "abc-123")]]
  (assert (has-flag-pair args "--resume" "abc-123") "build-args: --resume pair"))

# OpenCode basic args
(let [[args (agent:build-args {:backend :opencode} "hello" nil)]]
  (assert (= (first args) "opencode") "build-args: opencode binary")
  (assert (has-flag-pair args "--format" "json") "build-args: opencode format pair"))

# OpenCode with session resume
(let [[args (agent:build-args {:backend :opencode} "hi" "sess-1")]]
  (assert (has-flag-pair args "--session" "sess-1") "build-args: --session pair")
  (assert (any? (fn [x] (= x "--continue")) args) "build-args: --continue flag"))

# OpenCode model flag
(let [[args (agent:build-args {:backend :opencode :model "gpt-4"} "hi" nil)]]
  (assert (has-flag-pair args "-m" "gpt-4") "build-args: opencode -m pair"))

# Claude with all options
(let [[args (agent:build-args {:backend :claude
                                :model "opus"
                                :system-prompt "be helpful"
                                :allowed-tools ["Read" "Write"]
                                :denied-tools ["Bash"]
                                :skip-permissions true
                                :dir "/tmp/project"
                                :effort :high
                                :max-budget 1.5
                                :opts ["--extra"]}
                               "do stuff" nil)]]
  (assert (has-flag-pair args "--system-prompt" "be helpful") "build-args: system-prompt pair")
  (assert (has-flag-pair args "--allowedTools" "Read") "build-args: allowed Read")
  (assert (has-flag-pair args "--allowedTools" "Write") "build-args: allowed Write")
  (assert (has-flag-pair args "--disallowedTools" "Bash") "build-args: denied Bash")
  (assert (any? (fn [x] (= x "--dangerously-skip-permissions")) args) "build-args: skip perms")
  (assert (has-flag-pair args "--add-dir" "/tmp/project") "build-args: add-dir pair")
  (assert (has-flag-pair args "--effort" "high") "build-args: effort pair")
  (assert (has-flag-pair args "--max-budget-usd" "1.5") "build-args: max-budget pair")
  (assert (any? (fn [x] (= x "--extra")) args) "build-args: passthrough opt"))

# Pi basic args
(let [[args (agent:build-args {:backend :pi} "hello" nil)]]
  (assert (= (first args) "pi") "build-args: pi binary")
  (assert (has-flag-pair args "--mode" "json") "build-args: pi mode json")
  (assert (= (last args) "hello") "build-args: pi prompt is last")
  (assert (has-flag-pair args "-p" "hello") "build-args: pi -p prompt pair"))

# Pi with model
(let [[args (agent:build-args {:backend :pi :model "sonnet"} "hi" nil)]]
  (assert (has-flag-pair args "--model" "sonnet") "build-args: pi --model pair"))

# Pi with session resume
(let [[args (agent:build-args {:backend :pi} "hi" "pi-sess-1")]]
  (assert (has-flag-pair args "--session" "pi-sess-1") "build-args: pi --session pair")
  (assert (any? (fn [x] (= x "--continue")) args) "build-args: pi --continue flag"))

# Pi with system-prompt and effort (maps to --thinking)
(let [[args (agent:build-args {:backend :pi
                                :system-prompt "be brief"
                                :effort :high}
                               "task" nil)]]
  (assert (has-flag-pair args "--system-prompt" "be brief") "build-args: pi system-prompt")
  (assert (has-flag-pair args "--thinking" "high") "build-args: pi --thinking pair"))

# Pi with allowed-tools (comma-separated --tools)
(let [[args (agent:build-args {:backend :pi :allowed-tools ["read" "bash" "edit"]} "hi" nil)]]
  (assert (has-flag-pair args "--tools" "read,bash,edit") "build-args: pi --tools comma-separated"))

# Pi-only flags ignored for claude
(let [[args (agent:build-args {:backend :claude :allowed-tools ["Read"]} "hi" nil)]]
  (assert (not (any? (fn [x] (= x "--tools")) args)) "build-args: --tools not used for claude"))

# Unknown backend errors
(let [[[ok? err] (protect (agent:build-args {:backend :unknown} "hi" nil))]]
  (assert (not ok?) "build-args: unknown backend errors")
  (assert (= err:error :agent-error) "build-args: error is :agent-error"))

# Backend-specific flags omitted for other backend
(let [[args (agent:build-args {:backend :opencode :skip-permissions true} "hi" nil)]]
  (assert (not (any? (fn [x] (= x "--dangerously-skip-permissions")) args))
          "build-args: skip-permissions ignored for opencode"))


# ── make-handle ─────────────────────────────────────────────────────────────

(let [[h (agent:make-handle {:backend :claude :model "sonnet"})]]
  (assert (nil? h:session-id) "make-handle: session-id starts nil")
  (assert (= (get h:config :backend) :claude) "make-handle: config preserved")
  (assert (= (get h:config :model) "sonnet") "make-handle: model preserved"))


# ── send via mock subprocess (Claude) ───────────────────────────────────────

(def mock-script "tests/elle/agent-mock.lisp")

(file/write mock-script
  (string
    "(println (json/serialize {\"type\" \"content_block_delta\" \"delta\" {\"type\" \"text_delta\" \"text\" \"hello\"}}))\n"
    "(println (json/serialize {\"type\" \"content_block_delta\" \"delta\" {\"type\" \"text_delta\" \"text\" \" world\"}}))\n"
    "(println (json/serialize {\"type\" \"system\" \"data\" \"init\"}))\n"
    "(println (json/serialize {\"type\" \"result\" \"result\" \"hello world\" \"total_cost_usd\" 0.05 \"session_id\" \"sess-abc\" \"usage\" {\"input_tokens\" 100 \"output_tokens\" 50}}))\n"))

# stream/collect to consume the stream
(let* [[handle (agent:make-handle {:backend :claude
                                    :command [elle-bin mock-script]})]
       [chunks (stream/collect (agent:send handle "ignored"))]]

  (assert (= (length chunks) 3) "send claude: got 3 chunks (system skipped)")

  (let* [[c0 (first chunks)]
         [c1 (second chunks)]
         [c2 (last chunks)]]
    (assert (= c0:type :text) "send claude: chunk 0 is text")
    (assert (= c0:text "hello") "send claude: chunk 0 text")
    (assert (= c1:text " world") "send claude: chunk 1 text")
    (assert (= c2:type :result) "send claude: result type")
    (assert (= c2:cost 0.05) "send claude: result cost")
    (assert (= c2:session-id "sess-abc") "send claude: result session-id")
    (assert (= (get c2:tokens :input) 100) "send claude: input tokens")
    (assert (= (get c2:tokens :output) 50) "send claude: output tokens"))

  # Session-id written back to handle
  (assert (= handle:session-id "sess-abc") "send claude: handle session-id updated"))


# ── send via mock subprocess (OpenCode) ─────────────────────────────────────

(def oc-mock "tests/elle/agent-oc-mock.lisp")

(file/write oc-mock
  (string
    "(println (json/serialize {\"type\" \"text\" \"part\" {\"text\" \"thinking...\"}}))\n"
    "(println (json/serialize {\"type\" \"step_start\" \"data\" {}}))\n"
    "(println (json/serialize {\"type\" \"step_finish\" \"part\" {\"cost\" 0.03 \"tokens\" {:input 50 :output 25}}}))\n"))

(let* [[handle (agent:make-handle {:backend :opencode
                                    :command [elle-bin oc-mock]})]
       [chunks (stream/collect (agent:send handle "ignored"))]]

  (assert (= (length chunks) 2) "send opencode: got 2 chunks (step_start skipped)")

  (let* [[c0 (first chunks)]
         [c1 (last chunks)]]
    (assert (= c0:type :text) "send opencode: chunk 0 is text")
    (assert (= c0:text "thinking...") "send opencode: chunk 0 text")
    (assert (= c1:type :result) "send opencode: result type")
    (assert (= c1:cost 0.03) "send opencode: result cost")))

(file/delete oc-mock)


# ── send via mock subprocess (Pi) ───────────────────────────────────────────

(def pi-mock "tests/elle/agent-pi-mock.lisp")

(file/write pi-mock
  (string
    "(println (json/serialize {\"type\" \"session\" \"version\" 3 \"id\" \"pi-sess-1\" \"timestamp\" \"2025-01-01\" \"cwd\" \"/tmp\"}))\n"
    "(println (json/serialize {\"type\" \"message_update\" \"assistantMessageEvent\" {\"type\" \"text_delta\" \"contentIndex\" 1 \"delta\" \"hello\"}}))\n"
    "(println (json/serialize {\"type\" \"message_update\" \"assistantMessageEvent\" {\"type\" \"text_delta\" \"contentIndex\" 1 \"delta\" \" world\"}}))\n"
    "(println (json/serialize {\"type\" \"message_end\" \"message\" {\"role\" \"assistant\" \"usage\" {\"input\" 100 \"output\" 50 \"cost\" {\"total\" 0.04}}}}))\n"))

(let* [[handle (agent:make-handle {:backend :pi
                                    :command [elle-bin pi-mock]})]
       [chunks (stream/collect (agent:send handle "ignored"))]]

  (assert (= (length chunks) 3) "send pi: got 3 chunks (session skipped)")

  (let* [[c0 (first chunks)]
         [c1 (second chunks)]
         [c2 (last chunks)]]
    (assert (= c0:type :text) "send pi: chunk 0 is text")
    (assert (= c0:text "hello") "send pi: chunk 0 text")
    (assert (= c1:text " world") "send pi: chunk 1 text")
    (assert (= c2:type :result) "send pi: result type")
    (assert (= c2:cost 0.04) "send pi: result cost")
    (assert (= c2:session-id "pi-sess-1") "send pi: result session-id")
    (assert (= (get c2:tokens :input) 100) "send pi: input tokens")
    (assert (= (get c2:tokens :output) 50) "send pi: output tokens"))

  # Session-id written back to handle
  (assert (= handle:session-id "pi-sess-1") "send pi: handle session-id updated"))


# ── pi tool-use chunks ──────────────────────────────────────────────────────

(def pi-tool-mock "tests/elle/agent-pi-tool-mock.lisp")

(file/write pi-tool-mock
  (string
    "(println (json/serialize {\"type\" \"session\" \"id\" \"pi-tool-s\"}))\n"
    "(println (json/serialize {\"type\" \"message_update\" \"assistantMessageEvent\" {\"type\" \"toolcall_start\" \"contentIndex\" 0 \"partial\" {\"content\" [{\"type\" \"toolCall\" \"id\" \"tc_1\" \"name\" \"bash\"}]}}}))\n"
    "(println (json/serialize {\"type\" \"message_update\" \"assistantMessageEvent\" {\"type\" \"toolcall_delta\" \"contentIndex\" 0 \"delta\" \"{\\\"command\\\": \\\"ls\\\"}\"}}))\n"
    "(println (json/serialize {\"type\" \"message_update\" \"assistantMessageEvent\" {\"type\" \"text_delta\" \"contentIndex\" 1 \"delta\" \"done\"}}))\n"
    "(println (json/serialize {\"type\" \"message_end\" \"message\" {\"role\" \"assistant\" \"usage\" {\"input\" 10 \"output\" 5 \"cost\" {\"total\" 0.01}}}}))\n"))

(let* [[handle (agent:make-handle {:backend :pi
                                    :command [elle-bin pi-tool-mock]})]
       [chunks (stream/collect (agent:send handle "x"))]]
  (let* [[tool-uses  (filter (fn [c] (= c:type :tool-use)) chunks)]
         [tool-input (filter (fn [c] (= c:type :tool-input)) chunks)]
         [texts      (filter (fn [c] (= c:type :text)) chunks)]]
    (assert (= (length tool-uses) 1) "pi tool-use: got tool-use chunk")
    (assert (= (get (first tool-uses) :name) "bash") "pi tool-use: name is bash")
    (assert (= (get (first tool-uses) :id) "tc_1") "pi tool-use: id is tc_1")
    (assert (= (length tool-input) 1) "pi tool-use: got tool-input chunk")
    (assert (= (length texts) 1) "pi tool-use: got text chunk")))

(file/delete pi-tool-mock)


# ── pi send-collect ─────────────────────────────────────────────────────────

(let* [[handle (agent:make-handle {:backend :pi
                                    :command [elle-bin pi-mock]})]
       [result (agent:send-collect handle "x")]]
  (assert (= result:text "hello world") "pi send-collect: concatenated text")
  (assert (= result:cost 0.04) "pi send-collect: cost preserved")
  (assert (= result:session-id "pi-sess-1") "pi send-collect: session-id preserved"))


# ── pi multi-turn: session continuation ─────────────────────────────────────

(let [[handle (agent:make-handle {:backend :pi
                                   :command [elle-bin pi-mock]})]]
  (stream/for-each (fn [_] nil) (agent:send handle "first"))
  (assert (= handle:session-id "pi-sess-1") "pi multi-turn: session-id set")
  (let [[args (agent:build-args handle:config "second" handle:session-id)]]
    (assert (has-flag-pair args "--session" "pi-sess-1") "pi multi-turn: --session pair")
    (assert (any? (fn [x] (= x "--continue")) args) "pi multi-turn: --continue flag")))

(file/delete pi-mock)


# ── stream combinators work on send output ──────────────────────────────────

# stream/for-each — the primary use case
(let* [[handle (agent:make-handle {:backend :claude
                                    :command [elle-bin mock-script]})]
       [texts @[]]]
  (stream/for-each
    (fn [chunk]
      (when (= chunk:type :text)
        (push texts chunk:text)))
    (agent:send handle "ignored"))
  (assert (= (length texts) 2) "stream/for-each: got 2 text chunks")
  (assert (= (get texts 0) "hello") "stream/for-each: first text")
  (assert (= (get texts 1) " world") "stream/for-each: second text"))

# stream/filter + stream/collect
(let* [[handle (agent:make-handle {:backend :claude
                                    :command [elle-bin mock-script]})]
       [results (stream/collect
                  (stream/filter
                    (fn [c] (= c:type :result))
                    (agent:send handle "ignored")))]]
  (assert (= (length results) 1) "stream/filter: one result chunk")
  (assert (= (get (first results) :cost) 0.05) "stream/filter: result cost"))


# ── multi-turn: session continuation ────────────────────────────────────────

(let* [[handle (agent:make-handle {:backend :claude
                                    :command [elle-bin mock-script]})]]
  # First turn
  (stream/for-each (fn [_] nil) (agent:send handle "first turn"))
  (assert (= handle:session-id "sess-abc") "multi-turn: session-id set after first")

  # Verify build-args would include --resume for next turn
  (let [[args (agent:build-args handle:config "second turn" handle:session-id)]]
    (assert (has-flag-pair args "--resume" "sess-abc") "multi-turn: --resume pair")))


# ── send errors on nonzero exit without result ─────────────────────────────

(def fail-script "tests/elle/agent-fail.lisp")
(file/write fail-script "(sys/exit 1)\n")

(let* [[handle (agent:make-handle {:backend :claude
                                    :command [elle-bin fail-script]})]
       [[ok? err] (protect (stream/collect (agent:send handle "should fail")))]]
  (assert (not ok?) "send fail: errors on nonzero exit")
  (assert (= err:error :agent-error) "send fail: error is :agent-error"))

(file/delete fail-script)


# ── stderr chunks ───────────────────────────────────────────────────────────

(def stderr-mock "tests/elle/agent-stderr-mock.lisp")
(file/write stderr-mock
  (string
    "(eprintln \"warning: something\")\n"
    "(println (json/serialize {\"type\" \"content_block_delta\" \"delta\" {\"type\" \"text_delta\" \"text\" \"ok\"}}))\n"
    "(println (json/serialize {\"type\" \"result\" \"result\" \"ok\" \"total_cost_usd\" 0.01 \"session_id\" \"s2\" \"usage\" {\"input_tokens\" 1 \"output_tokens\" 1}}))\n"))

(let* [[handle (agent:make-handle {:backend :claude
                                    :command [elle-bin stderr-mock]})]
       [chunks  (stream/collect (agent:send handle "x"))]
       [texts   (filter (fn [c] (= c:type :text)) chunks)]]
  (assert (> (length texts) 0) "stderr: got text chunks")
  (assert (= (get (first texts) :text) "ok") "stderr: text content"))

(file/delete stderr-mock)


# ── tool-use chunks (Claude) ───────────────────────────────────────────────

(def tool-mock "tests/elle/agent-tool-mock.lisp")
(file/write tool-mock
  (string
    "(println (json/serialize {\"type\" \"content_block_start\" \"content_block\" {\"type\" \"tool_use\" \"name\" \"Read\" \"id\" \"tu_1\"}}))\n"
    "(println (json/serialize {\"type\" \"content_block_delta\" \"delta\" {\"type\" \"input_json_delta\" \"partial_json\" \"{\\\"path\\\":\"}}))\n"
    "(println (json/serialize {\"type\" \"content_block_delta\" \"delta\" {\"type\" \"text_delta\" \"text\" \"reading file\"}}))\n"
    "(println (json/serialize {\"type\" \"result\" \"result\" \"done\" \"total_cost_usd\" 0.02 \"session_id\" \"s3\" \"usage\" {\"input_tokens\" 10 \"output_tokens\" 5}}))\n"))

(let* [[handle (agent:make-handle {:backend :claude
                                    :command [elle-bin tool-mock]})]
       [chunks (stream/collect (agent:send handle "x"))]]
  (let* [[tool-uses  (filter (fn [c] (= c:type :tool-use)) chunks)]
         [tool-input (filter (fn [c] (= c:type :tool-input)) chunks)]
         [texts      (filter (fn [c] (= c:type :text)) chunks)]]
    (assert (= (length tool-uses) 1) "tool-use: got tool-use chunk")
    (assert (= (get (first tool-uses) :name) "Read") "tool-use: name is Read")
    (assert (= (get (first tool-uses) :id) "tu_1") "tool-use: id is tu_1")
    (assert (= (length tool-input) 1) "tool-use: got tool-input chunk")
    (assert (= (length texts) 1) "tool-use: got text chunk")))

(file/delete tool-mock)


# ── send-collect ────────────────────────────────────────────────────────────

(let* [[handle (agent:make-handle {:backend :claude
                                    :command [elle-bin mock-script]})]
       [result (agent:send-collect handle "x")]]
  (assert (= result:text "hello world") "send-collect: concatenated text")
  (assert (= result:cost 0.05) "send-collect: cost preserved")
  (assert (= result:session-id "sess-abc") "send-collect: session-id preserved"))


# ── total-cost accumulation ─────────────────────────────────────────────────

(let [[handle (agent:make-handle {:backend :claude
                                   :command [elle-bin mock-script]})]]
  (assert (= handle:total-cost 0) "total-cost: starts at 0")
  (stream/for-each (fn [_] nil) (agent:send handle "turn 1"))
  (assert (= handle:total-cost 0.05) "total-cost: after turn 1")
  (stream/for-each (fn [_] nil) (agent:send handle "turn 2"))
  (assert (= handle:total-cost 0.1) "total-cost: after turn 2"))


# ── kill ────────────────────────────────────────────────────────────────────

(def slow-mock "tests/elle/agent-slow-mock.lisp")
(file/write slow-mock
  (string
    "(println (json/serialize {\"type\" \"content_block_delta\" \"delta\" {\"type\" \"text_delta\" \"text\" \"start\"}}))\n"
    "(port/flush (port/stdout))\n"
    "(time/sleep 60)\n"))

(let [[handle (agent:make-handle {:backend :claude
                                   :command [elle-bin slow-mock]})]]
  # Consume one chunk — proc should be set, then kill
  (let [[co (agent:send handle "x")]]
    (coro/resume co)
    (assert (not (nil? handle:proc)) "kill: proc set during send")
    (agent:kill handle)
    (assert (nil? handle:proc) "kill: proc cleared after kill")))

(file/delete slow-mock)


# ── proc cleared after normal send ──────────────────────────────────────────

(let [[handle (agent:make-handle {:backend :claude
                                   :command [elle-bin mock-script]})]]
  (stream/for-each (fn [_] nil) (agent:send handle "x"))
  (assert (nil? handle:proc) "proc: cleared after normal completion"))


# ── chunk contract validation ────────────────────────────────────────────────

# Bad chunk: :cost is string instead of number
(def bad-cost-mock "tests/elle/agent-bad-cost.lisp")
(file/write bad-cost-mock
  (string
    "(println (json/serialize {\"type\" \"step_finish\" \"part\" {\"cost\" \"expensive\"}}))"))

(let* [[handle (agent:make-handle {:backend :opencode
                                    :command [elle-bin bad-cost-mock]})]
       [[ok? err] (protect (stream/collect (agent:send handle "x")))]]
  (assert (not ok?) "contract: non-number cost rejected")
  (assert (= err:error :agent-error) "contract: cost error is :agent-error"))

(file/delete bad-cost-mock)

# Bad chunk: :text is integer instead of string
(def bad-text-mock "tests/elle/agent-bad-text.lisp")
(file/write bad-text-mock
  (string
    "(println (json/serialize {\"type\" \"content_block_delta\" \"delta\" {\"type\" \"text_delta\" \"text\" 42}}))\n"
    "(println (json/serialize {\"type\" \"result\" \"result\" \"ok\" \"total_cost_usd\" 0.01 \"session_id\" \"s\" \"usage\" {\"input_tokens\" 1 \"output_tokens\" 1}}))"))

(let* [[handle (agent:make-handle {:backend :claude
                                    :command [elle-bin bad-text-mock]})]
       [[ok? err] (protect (stream/collect (agent:send handle "x")))]]
  (assert (not ok?) "contract: non-string text rejected")
  (assert (= err:error :agent-error) "contract: text error is :agent-error"))

(file/delete bad-text-mock)


# ── cleanup ─────────────────────────────────────────────────────────────────

(file/delete mock-script)

(eprintln "all agent tests passed")
